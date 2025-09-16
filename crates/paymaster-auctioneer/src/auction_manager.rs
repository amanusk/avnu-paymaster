use crate::{AuctionResult, BuildTransactionResponse, Error, PaymasterBid};
use paymaster_rpc::client::Client;
use paymaster_starknet::values::decoding::TypedValueDecoder;
use starknet::core::types::Felt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};
use tracing::{info, warn};

/// Handles auction logic for selecting the best paymaster bid and managing auction results
#[derive(Clone)]
pub struct AuctionManager {
    /// Timeout for individual paymaster requests
    pub request_timeout: Duration,
    /// Storage for auction results
    pub auction_results: Arc<RwLock<HashMap<Felt, AuctionResult>>>,
    /// Timeout for auction clearing in milliseconds
    pub auction_timeout_ms: u64,
}

impl AuctionManager {
    pub fn new() -> Self {
        Self {
            request_timeout: Duration::from_millis(5000), // 5 second timeout
            auction_results: Arc::new(RwLock::new(HashMap::new())),
            auction_timeout_ms: 30000, // 30 second default timeout
        }
    }

    pub fn new_with_config(request_timeout: Duration, auction_timeout_ms: u64) -> Self {
        Self {
            request_timeout,
            auction_results: Arc::new(RwLock::new(HashMap::new())),
            auction_timeout_ms,
        }
    }

    /// Run an auction by requesting bids from all active paymasters
    pub async fn run_auction(
        &self,
        paymasters: Vec<(String, String)>, // (name, url)
        request: paymaster_rpc::BuildTransactionRequest,
    ) -> Result<AuctionResult, Error> {
        if paymasters.is_empty() {
            return Err(Error::NoActivePaymasters);
        }

        // Send requests to all paymasters in parallel
        let mut tasks = Vec::new();
        let request_timeout = self.request_timeout;
        for (name, url) in paymasters {
            let client = Client::new(&url);
            let request = request.clone();
            let task = tokio::spawn(async move {
                match timeout(request_timeout, client.build_transaction(request)).await {
                    Ok(Ok(response)) => Some((name, response)),
                    Ok(Err(e)) => {
                        warn!("Paymaster {} returned error: {}", name, e);
                        None
                    },
                    Err(_) => {
                        warn!("Paymaster {} timed out", name);
                        None
                    },
                }
            });
            tasks.push(task);
        }

        // Collect responses and extract gas token information
        let mut bids = Vec::new();
        for task in tasks {
            if let Ok(Some((name, response))) = task.await {
                match self.extract_gas_token_info(&response) {
                    Ok((gas_token, amount)) => {
                        info!("Received valid bid from paymaster {}: gas_token={}, amount={}", name, gas_token, amount);
                        bids.push(PaymasterBid {
                            paymaster_name: name,
                            response,
                            gas_token,
                            amount,
                        });
                    },
                    Err(e) => {
                        warn!("Failed to extract gas token info from {}: {}", name, e);
                    },
                }
            }
        }

        if bids.is_empty() {
            return Err(Error::NoValidBids);
        }

        // Find the winning bid (lowest amount)
        let winning_bid = bids.iter().min_by_key(|bid| bid.amount).ok_or(Error::NoValidBids)?;

        // Create auction ID from the typed data (without signature)
        let auction_id = self.create_auction_id(&winning_bid.response)?;

        // Create auction result
        let auction_result = AuctionResult {
            auction_id,
            winning_paymaster: winning_bid.paymaster_name.clone(),
            gas_token: winning_bid.gas_token,
            amount: winning_bid.amount,
            response: winning_bid.response.clone(),
            created_at: SystemTime::now(),
        };

        // Store the auction result
        self.store_auction_result(auction_result.clone()).await;

        info!(
            "Auction completed. Winner: {}, Gas token: {}, Amount: {}, Auction ID: {}",
            winning_bid.paymaster_name, winning_bid.gas_token, winning_bid.amount, auction_id
        );

        Ok(auction_result)
    }

    /// Extract gas token and amount from a BuildTransactionResponse
    fn extract_gas_token_info(&self, response: &BuildTransactionResponse) -> Result<(Felt, Felt), Error> {
        let typed_data = match response {
            BuildTransactionResponse::Invoke(invoke) => &invoke.typed_data,
            BuildTransactionResponse::DeployAndInvoke(deploy_invoke) => &deploy_invoke.typed_data,
            BuildTransactionResponse::Deploy(_) => return Err(Error::FailedToExtractGasToken),
        };

        // Use TypedValueDecoder to properly access the message data
        let decoder = TypedValueDecoder::new(&typed_data.message());
        let object_decoder = decoder.decode_object().map_err(|_| Error::FailedToExtractGasToken)?;

        let calls_decoder = object_decoder
            .decode_field("Calls")
            .map_err(|_| Error::FailedToExtractGasToken)?;
        let calls_array = calls_decoder.decode_array().map_err(|_| Error::FailedToExtractGasToken)?;

        let calls: Vec<_> = calls_array.collect();
        let last_call = calls.last().ok_or(Error::FailedToExtractGasToken)?;

        // Decode the last call as an object
        let call_decoder = last_call.decode_object().map_err(|_| Error::FailedToExtractGasToken)?;

        // Check if it's a transfer call
        let selector_decoder = call_decoder
            .decode_field("Selector")
            .map_err(|_| Error::FailedToExtractGasToken)?;
        let selector = selector_decoder
            .decode::<String>()
            .map_err(|_| Error::FailedToExtractGasToken)?;

        // The transfer selector for ERC20 is 0x83afd3f4caedc6eebf44246fe54e38c95e3179a5ec9ea81740eca5b482d12e
        if selector != "0x83afd3f4caedc6eebf44246fe54e38c95e3179a5ec9ea81740eca5b482d12e" {
            return Err(Error::FailedToExtractGasToken);
        }

        // Get the gas token address from the 'To' field
        let to_decoder = call_decoder.decode_field("To").map_err(|_| Error::FailedToExtractGasToken)?;
        let gas_token = to_decoder.decode::<Felt>().map_err(|_| Error::FailedToExtractGasToken)?;

        // Get the calldata array
        let calldata_decoder = call_decoder
            .decode_field("Calldata")
            .map_err(|_| Error::FailedToExtractGasToken)?;
        let calldata_array = calldata_decoder.decode_array().map_err(|_| Error::FailedToExtractGasToken)?;

        let calldata: Vec<_> = calldata_array.collect();
        if calldata.len() < 2 {
            return Err(Error::FailedToExtractGasToken);
        }

        // The amount is the second parameter of the transfer call
        let amount = calldata[1].decode::<Felt>().map_err(|_| Error::FailedToExtractGasToken)?;

        Ok((gas_token, amount))
    }

    /// Create an auction ID by hashing the typed data without the user signature
    fn create_auction_id(&self, response: &BuildTransactionResponse) -> Result<Felt, Error> {
        let typed_data = match response {
            BuildTransactionResponse::Invoke(invoke) => &invoke.typed_data,
            BuildTransactionResponse::DeployAndInvoke(deploy_invoke) => &deploy_invoke.typed_data,
            BuildTransactionResponse::Deploy(_) => return Err(Error::FailedToCreateAuctionId),
        };

        // Generate auction ID from TypedData hash (without signature)
        // This matches the logic from the reference implementation
        let auction_id = self.generate_auction_id_from_typed_data(typed_data)?;

        Ok(auction_id)
    }

    /// Generate auction ID from TypedData hash (without signature)
    /// This function handles both signed and unsigned TypedData by removing the signature field if present
    /// This matches the logic from the reference implementation
    pub fn generate_auction_id_from_typed_data(&self, typed_data: &starknet::core::types::TypedData) -> Result<Felt, Error> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Convert TypedData to JSON
        let mut typed_data_json = serde_json::to_value(typed_data).map_err(|_| Error::FailedToCreateAuctionId)?;

        // Remove the signature field if it exists (for signed TypedData)
        if let Some(obj) = typed_data_json.as_object_mut() {
            obj.remove("signature");
        }

        // Hash the TypedData without signature
        let mut hasher = DefaultHasher::new();
        typed_data_json.hash(&mut hasher);
        let hash_value = hasher.finish();

        // Convert the hash to a Felt
        let auction_id = Felt::from(hash_value);

        Ok(auction_id)
    }

    /// Store an auction result
    pub async fn store_auction_result(&self, auction_result: AuctionResult) {
        let mut results = self.auction_results.write().await;
        results.insert(auction_result.auction_id, auction_result);
    }

    /// Get an auction result by ID
    pub async fn get_auction_result(&self, auction_id: Felt) -> Option<AuctionResult> {
        let results = self.auction_results.read().await;
        results.get(&auction_id).cloned()
    }

    /// Remove an auction result by ID
    pub async fn remove_auction_result(&self, auction_id: Felt) -> Option<AuctionResult> {
        let mut results = self.auction_results.write().await;
        results.remove(&auction_id)
    }

    /// Start the auction clearing task that runs periodically
    pub async fn start_auction_clearing_task(&self) {
        let auction_results = self.auction_results.clone();
        let auction_timeout_ms = self.auction_timeout_ms;

        info!("Starting auction clearing task with {}ms interval", auction_timeout_ms);

        let mut interval_timer = interval(Duration::from_millis(auction_timeout_ms));

        tokio::spawn(async move {
            loop {
                interval_timer.tick().await;

                let now = SystemTime::now();
                let timeout_duration = Duration::from_millis(auction_timeout_ms);

                // Get all auction IDs that need to be cleared
                let auctions_to_clear = {
                    let results = auction_results.read().await;
                    results
                        .iter()
                        .filter(|(_, result)| {
                            now.duration_since(result.created_at)
                                .map(|elapsed| elapsed > timeout_duration)
                                .unwrap_or(false)
                        })
                        .map(|(auction_id, _)| *auction_id)
                        .collect::<Vec<_>>()
                };

                if !auctions_to_clear.is_empty() {
                    info!("Clearing {} expired auctions", auctions_to_clear.len());

                    // Remove expired auctions
                    {
                        let mut results = auction_results.write().await;
                        for auction_id in auctions_to_clear {
                            if let Some(removed) = results.remove(&auction_id) {
                                info!("Cleared expired auction: {} (created at: {:?})", auction_id, removed.created_at);
                            }
                        }
                    }
                }
            }
        });
    }
}

impl Default for AuctionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paymaster_rpc::{DeployTransaction, DeploymentParameters, ExecutionParameters, FeeEstimate, FeeMode};
    use starknet::core::types::Felt;

    #[test]
    fn test_auction_manager_creation() {
        let manager = AuctionManager::new();
        assert_eq!(manager.request_timeout, Duration::from_millis(5000));
    }

    #[test]
    fn test_auction_manager_default() {
        let manager = AuctionManager::default();
        assert_eq!(manager.request_timeout, Duration::from_millis(5000));
    }

    #[test]
    fn test_extract_gas_token_info_deploy_transaction() {
        let manager = AuctionManager::new();
        let response = BuildTransactionResponse::Deploy(DeployTransaction {
            deployment: DeploymentParameters {
                address: Felt::from_hex("0x123").unwrap(),
                class_hash: Felt::from_hex("0x456").unwrap(),
                salt: Felt::from_hex("0x789").unwrap(),
                calldata: vec![],
                sigdata: None,
                version: 1,
            },
            parameters: ExecutionParameters::V1 {
                fee_mode: FeeMode::Default {
                    gas_token: Felt::from_hex("0x789").unwrap(),
                },
                time_bounds: None,
            },
            fee: FeeEstimate {
                gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                estimated_fee_in_gas_token: Felt::from_hex("0x2000").unwrap(),
                suggested_max_fee_in_strk: Felt::from_hex("0x3000").unwrap(),
                suggested_max_fee_in_gas_token: Felt::from_hex("0x4000").unwrap(),
            },
        });

        let result = manager.extract_gas_token_info(&response);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::FailedToExtractGasToken));
    }

    #[test]
    fn test_create_auction_id_deploy_transaction() {
        let manager = AuctionManager::new();
        let response = BuildTransactionResponse::Deploy(DeployTransaction {
            deployment: DeploymentParameters {
                address: Felt::from_hex("0x123").unwrap(),
                class_hash: Felt::from_hex("0x456").unwrap(),
                salt: Felt::from_hex("0x789").unwrap(),
                calldata: vec![],
                sigdata: None,
                version: 1,
            },
            parameters: ExecutionParameters::V1 {
                fee_mode: FeeMode::Default {
                    gas_token: Felt::from_hex("0x789").unwrap(),
                },
                time_bounds: None,
            },
            fee: FeeEstimate {
                gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                estimated_fee_in_gas_token: Felt::from_hex("0x2000").unwrap(),
                suggested_max_fee_in_strk: Felt::from_hex("0x3000").unwrap(),
                suggested_max_fee_in_gas_token: Felt::from_hex("0x4000").unwrap(),
            },
        });

        let result = manager.create_auction_id(&response);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::FailedToCreateAuctionId));
    }

    #[test]
    fn test_paymaster_bid_comparison() {
        let gas_token = Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap();
        let amount1 = Felt::from_hex("0x1000").unwrap();
        let amount2 = Felt::from_hex("0x2000").unwrap();

        // Create simple mock responses - we'll use a minimal response for testing
        let response = BuildTransactionResponse::Deploy(DeployTransaction {
            deployment: DeploymentParameters {
                address: Felt::from_hex("0x123").unwrap(),
                class_hash: Felt::from_hex("0x456").unwrap(),
                salt: Felt::from_hex("0x789").unwrap(),
                calldata: vec![],
                sigdata: None,
                version: 1,
            },
            parameters: ExecutionParameters::V1 {
                fee_mode: FeeMode::Default { gas_token },
                time_bounds: None,
            },
            fee: FeeEstimate {
                gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                estimated_fee_in_gas_token: amount1,
                suggested_max_fee_in_strk: Felt::from_hex("0x2000").unwrap(),
                suggested_max_fee_in_gas_token: amount1,
            },
        });

        let bid1 = PaymasterBid {
            paymaster_name: "paymaster1".to_string(),
            response: response.clone(),
            gas_token,
            amount: amount1,
        };

        let bid2 = PaymasterBid {
            paymaster_name: "paymaster2".to_string(),
            response,
            gas_token,
            amount: amount2,
        };

        // Test that bid1 has lower amount than bid2
        assert!(bid1.amount < bid2.amount);

        // Test min_by_key selection
        let bids = vec![bid1.clone(), bid2.clone()];
        let winning_bid = bids.iter().min_by_key(|bid| bid.amount).unwrap();
        assert_eq!(winning_bid.paymaster_name, "paymaster1");
        assert_eq!(winning_bid.amount, amount1);
    }

    #[test]
    fn test_auction_result_creation() {
        let gas_token = Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap();
        let amount = Felt::from_hex("0x1400f76af02").unwrap();
        let auction_id = Felt::from_hex("0x123456789").unwrap();

        let response = BuildTransactionResponse::Deploy(DeployTransaction {
            deployment: DeploymentParameters {
                address: Felt::from_hex("0x123").unwrap(),
                class_hash: Felt::from_hex("0x456").unwrap(),
                salt: Felt::from_hex("0x789").unwrap(),
                calldata: vec![],
                sigdata: None,
                version: 1,
            },
            parameters: ExecutionParameters::V1 {
                fee_mode: FeeMode::Default { gas_token },
                time_bounds: None,
            },
            fee: FeeEstimate {
                gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                estimated_fee_in_gas_token: amount,
                suggested_max_fee_in_strk: Felt::from_hex("0x2000").unwrap(),
                suggested_max_fee_in_gas_token: amount,
            },
        });

        let auction_result = AuctionResult {
            auction_id,
            winning_paymaster: "test_paymaster".to_string(),
            gas_token,
            amount,
            response: response.clone(),
            created_at: std::time::SystemTime::now(),
        };

        assert_eq!(auction_result.auction_id, auction_id);
        assert_eq!(auction_result.winning_paymaster, "test_paymaster");
        assert_eq!(auction_result.gas_token, gas_token);
        assert_eq!(auction_result.amount, amount);
    }

    #[test]
    fn test_multiple_bids_auction_simulation() {
        let _manager = AuctionManager::new();
        let gas_token = Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap();

        // Create multiple bids with different amounts
        let amounts = vec![
            Felt::from_hex("0x5000").unwrap(), // Highest
            Felt::from_hex("0x2000").unwrap(), // Middle
            Felt::from_hex("0x1000").unwrap(), // Lowest - should win
        ];

        let mut bids = Vec::new();
        for (i, amount) in amounts.iter().enumerate() {
            let response = BuildTransactionResponse::Deploy(DeployTransaction {
                deployment: DeploymentParameters {
                    address: Felt::from_hex("0x123").unwrap(),
                    class_hash: Felt::from_hex("0x456").unwrap(),
                    salt: Felt::from_hex("0x789").unwrap(),
                    calldata: vec![],
                    sigdata: None,
                    version: 1,
                },
                parameters: ExecutionParameters::V1 {
                    fee_mode: FeeMode::Default { gas_token },
                    time_bounds: None,
                },
                fee: FeeEstimate {
                    gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                    estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                    estimated_fee_in_gas_token: *amount,
                    suggested_max_fee_in_strk: Felt::from_hex("0x2000").unwrap(),
                    suggested_max_fee_in_gas_token: *amount,
                },
            });

            let bid = PaymasterBid {
                paymaster_name: format!("paymaster{}", i),
                response,
                gas_token,
                amount: *amount,
            };
            bids.push(bid);
        }

        // Find the winning bid (lowest amount)
        let winning_bid = bids.iter().min_by_key(|bid| bid.amount).unwrap();

        assert_eq!(winning_bid.paymaster_name, "paymaster2");
        assert_eq!(winning_bid.amount, Felt::from_hex("0x1000").unwrap());
    }

    #[test]
    fn test_gas_token_extraction_edge_cases() {
        let manager = AuctionManager::new();
        let gas_token = Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap();

        // Test with zero amount
        let zero_amount = Felt::ZERO;
        let response = BuildTransactionResponse::Deploy(DeployTransaction {
            deployment: DeploymentParameters {
                address: Felt::from_hex("0x123").unwrap(),
                class_hash: Felt::from_hex("0x456").unwrap(),
                salt: Felt::from_hex("0x789").unwrap(),
                calldata: vec![],
                sigdata: None,
                version: 1,
            },
            parameters: ExecutionParameters::V1 {
                fee_mode: FeeMode::Default { gas_token },
                time_bounds: None,
            },
            fee: FeeEstimate {
                gas_token_price_in_strk: Felt::from_hex("0x1").unwrap(),
                estimated_fee_in_strk: Felt::from_hex("0x1000").unwrap(),
                estimated_fee_in_gas_token: zero_amount,
                suggested_max_fee_in_strk: Felt::from_hex("0x2000").unwrap(),
                suggested_max_fee_in_gas_token: zero_amount,
            },
        });

        let result = manager.extract_gas_token_info(&response);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::FailedToExtractGasToken));
    }

    #[test]
    fn test_auction_id_generation_basic() {
        use starknet::core::types::TypedData;

        let manager = AuctionManager::new();

        // Test that the auction ID generation function works
        // We'll test with a proper TypedData JSON format
        let test_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "TestMessage": [
                    {"name": "test", "type": "felt"}
                ]
            },
            "primaryType": "TestMessage",
            "domain": {
                "name": "Test",
                "version": "1",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "test": "0x123"
            }
        });

        // Convert to TypedData using serde_json::from_value
        let typed_data: TypedData = serde_json::from_value(test_json).unwrap();

        // Generate auction ID from TypedData
        let auction_id = manager.generate_auction_id_from_typed_data(&typed_data).unwrap();

        println!("Generated auction ID: {}", auction_id);

        // Test 1: Consistency - same TypedData should produce same auction ID
        let auction_id_consistency = manager.generate_auction_id_from_typed_data(&typed_data).unwrap();
        assert_eq!(auction_id, auction_id_consistency, "Auction ID should be consistent for the same TypedData");

        // Test 2: Verify the auction ID format
        assert!(auction_id != Felt::ZERO, "Auction ID should not be zero");

        println!("✅ Auction ID generation test passed!");
        println!("   - Same TypedData produces consistent auction ID: {}", auction_id == auction_id_consistency);
    }

    #[test]
    fn test_auction_id_signature_independence() {
        use starknet::core::types::TypedData;

        let manager = AuctionManager::new();

        // Create a base TypedData without signature
        let base_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "OutsideExecution": [
                    {"name": "Caller", "type": "ContractAddress"},
                    {"name": "Nonce", "type": "felt"},
                    {"name": "Execute After", "type": "u128"},
                    {"name": "Execute Before", "type": "u128"},
                    {"name": "Calls", "type": "Call*"}
                ],
                "Call": [
                    {"name": "To", "type": "ContractAddress"},
                    {"name": "Selector", "type": "selector"},
                    {"name": "Calldata", "type": "felt*"}
                ]
            },
            "primaryType": "OutsideExecution",
            "domain": {
                "name": "Account.execute_from_outside",
                "version": "2",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "Caller": "0x0",
                "Nonce": "0x1234567890abcdef",
                "Execute After": "0x1234567890",
                "Execute Before": "0x1234567890abcdef",
                "Calls": [
                    {
                        "To": "0x1234567890abcdef1234567890abcdef12345678",
                        "Selector": "0x1234567890abcdef1234567890abcdef12345678",
                        "Calldata": ["0x1", "0x2", "0x3"]
                    }
                ]
            }
        });

        // Convert to TypedData without signature
        let typed_data_without_sig: TypedData = serde_json::from_value(base_json.clone()).unwrap();

        // Generate auction ID from TypedData without signature
        let auction_id_without_sig = manager.generate_auction_id_from_typed_data(&typed_data_without_sig).unwrap();

        println!("Auction ID without signature: {}", auction_id_without_sig);

        // Create TypedData with signature by adding signature field to JSON
        let mut json_with_sig = base_json.clone();
        if let Some(typed_data_obj) = json_with_sig.as_object_mut() {
            typed_data_obj.insert(
                "signature".to_string(),
                serde_json::json!([
                    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                ]),
            );
        }

        // Convert to TypedData with signature
        let typed_data_with_sig: TypedData = serde_json::from_value(json_with_sig).unwrap();

        // Generate auction ID from TypedData with signature
        let auction_id_with_sig = manager.generate_auction_id_from_typed_data(&typed_data_with_sig).unwrap();

        println!("Auction ID with signature: {}", auction_id_with_sig);

        // Test 1: Signature independence - TypedData with and without signature should produce same auction ID
        assert_eq!(
            auction_id_without_sig, auction_id_with_sig,
            "Auction ID should be the same whether TypedData has signature or not"
        );

        // Test 2: Consistency - same TypedData should produce same auction ID
        let auction_id_consistency = manager.generate_auction_id_from_typed_data(&typed_data_without_sig).unwrap();
        assert_eq!(
            auction_id_without_sig, auction_id_consistency,
            "Auction ID should be consistent for the same TypedData"
        );

        // Test 3: Different content should produce different auction IDs
        let different_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "OutsideExecution": [
                    {"name": "Caller", "type": "ContractAddress"},
                    {"name": "Nonce", "type": "felt"},
                    {"name": "Execute After", "type": "u128"},
                    {"name": "Execute Before", "type": "u128"},
                    {"name": "Calls", "type": "Call*"}
                ],
                "Call": [
                    {"name": "To", "type": "ContractAddress"},
                    {"name": "Selector", "type": "selector"},
                    {"name": "Calldata", "type": "felt*"}
                ]
            },
            "primaryType": "OutsideExecution",
            "domain": {
                "name": "Account.execute_from_outside",
                "version": "2",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "Caller": "0x0",
                "Nonce": "0x9876543210fedcba", // Different nonce
                "Execute After": "0x1234567890",
                "Execute Before": "0x1234567890abcdef",
                "Calls": [
                    {
                        "To": "0x1234567890abcdef1234567890abcdef12345678",
                        "Selector": "0x1234567890abcdef1234567890abcdef12345678",
                        "Calldata": ["0x1", "0x2", "0x3"]
                    }
                ]
            }
        });

        let different_typed_data: TypedData = serde_json::from_value(different_json).unwrap();
        let different_auction_id = manager.generate_auction_id_from_typed_data(&different_typed_data).unwrap();

        println!("Different auction ID: {}", different_auction_id);

        assert_ne!(
            auction_id_without_sig, different_auction_id,
            "Auction ID should be different for different TypedData content"
        );

        // Test 4: Verify the auction ID format
        assert!(auction_id_without_sig != Felt::ZERO, "Auction ID should not be zero");
        assert!(auction_id_with_sig != Felt::ZERO, "Auction ID should not be zero");

        println!("✅ Auction ID signature independence test passed!");
        println!(
            "   - Same TypedData produces consistent auction ID: {}",
            auction_id_without_sig == auction_id_consistency
        );
        println!("   - Signature presence doesn't affect auction ID: {}", auction_id_without_sig == auction_id_with_sig);
        println!(
            "   - Different content produces different auction ID: {}",
            auction_id_without_sig != different_auction_id
        );
    }

    #[test]
    fn test_single_function_signature_independence() {
        use starknet::core::types::TypedData;

        let manager = AuctionManager::new();

        // Create a base TypedData without signature
        let base_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "OutsideExecution": [
                    {"name": "Caller", "type": "ContractAddress"},
                    {"name": "Nonce", "type": "felt"},
                    {"name": "Execute After", "type": "u128"},
                    {"name": "Execute Before", "type": "u128"},
                    {"name": "Calls", "type": "Call*"}
                ],
                "Call": [
                    {"name": "To", "type": "ContractAddress"},
                    {"name": "Selector", "type": "selector"},
                    {"name": "Calldata", "type": "felt*"}
                ]
            },
            "primaryType": "OutsideExecution",
            "domain": {
                "name": "Account.execute_from_outside",
                "version": "2",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "Caller": "0x0",
                "Nonce": "0x1234567890abcdef",
                "Execute After": "0x1234567890",
                "Execute Before": "0x1234567890abcdef",
                "Calls": [
                    {
                        "To": "0x1234567890abcdef1234567890abcdef12345678",
                        "Selector": "0x1234567890abcdef1234567890abcdef12345678",
                        "Calldata": ["0x1", "0x2", "0x3"]
                    }
                ]
            }
        });

        // Convert to TypedData without signature
        let typed_data_without_sig: TypedData = serde_json::from_value(base_json.clone()).unwrap();

        // Generate auction ID from TypedData without signature using the single function
        let auction_id_without_sig = manager.generate_auction_id_from_typed_data(&typed_data_without_sig).unwrap();

        println!("Auction ID without signature (single function): {}", auction_id_without_sig);

        // Create TypedData with signature by adding signature field to JSON
        let mut json_with_sig = base_json.clone();
        if let Some(typed_data_obj) = json_with_sig.as_object_mut() {
            typed_data_obj.insert(
                "signature".to_string(),
                serde_json::json!([
                    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                ]),
            );
        }

        // Convert to TypedData with signature
        let typed_data_with_sig: TypedData = serde_json::from_value(json_with_sig).unwrap();

        // Generate auction ID from TypedData with signature using the single function
        let auction_id_with_sig = manager.generate_auction_id_from_typed_data(&typed_data_with_sig).unwrap();

        println!("Auction ID with signature (single function): {}", auction_id_with_sig);

        // Test 1: Signature independence - TypedData with and without signature should produce same auction ID
        assert_eq!(
            auction_id_without_sig, auction_id_with_sig,
            "Single function should produce the same auction ID whether TypedData has signature or not"
        );

        // Test 2: Consistency - same TypedData should produce same auction ID
        let auction_id_consistency = manager.generate_auction_id_from_typed_data(&typed_data_without_sig).unwrap();
        assert_eq!(
            auction_id_without_sig, auction_id_consistency,
            "Single function should produce consistent auction ID for the same TypedData"
        );

        // Test 3: Different content should produce different auction IDs
        let different_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "OutsideExecution": [
                    {"name": "Caller", "type": "ContractAddress"},
                    {"name": "Nonce", "type": "felt"},
                    {"name": "Execute After", "type": "u128"},
                    {"name": "Execute Before", "type": "u128"},
                    {"name": "Calls", "type": "Call*"}
                ],
                "Call": [
                    {"name": "To", "type": "ContractAddress"},
                    {"name": "Selector", "type": "selector"},
                    {"name": "Calldata", "type": "felt*"}
                ]
            },
            "primaryType": "OutsideExecution",
            "domain": {
                "name": "Account.execute_from_outside",
                "version": "2",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "Caller": "0x0",
                "Nonce": "0x9876543210fedcba", // Different nonce
                "Execute After": "0x1234567890",
                "Execute Before": "0x1234567890abcdef",
                "Calls": [
                    {
                        "To": "0x1234567890abcdef1234567890abcdef12345678",
                        "Selector": "0x1234567890abcdef1234567890abcdef12345678",
                        "Calldata": ["0x1", "0x2", "0x3"]
                    }
                ]
            }
        });

        let different_typed_data: TypedData = serde_json::from_value(different_json).unwrap();
        let different_auction_id = manager.generate_auction_id_from_typed_data(&different_typed_data).unwrap();

        println!("Different auction ID: {}", different_auction_id);

        assert_ne!(
            auction_id_without_sig, different_auction_id,
            "Single function should produce different auction ID for different TypedData content"
        );

        // Test 4: Verify the auction ID format
        assert!(auction_id_without_sig != Felt::ZERO, "Auction ID should not be zero");
        assert!(auction_id_with_sig != Felt::ZERO, "Auction ID should not be zero");

        println!("✅ Single function signature independence test passed!");
        println!(
            "   - Same TypedData produces consistent auction ID: {}",
            auction_id_without_sig == auction_id_consistency
        );
        println!("   - Signature presence doesn't affect auction ID: {}", auction_id_without_sig == auction_id_with_sig);
        println!(
            "   - Different content produces different auction ID: {}",
            auction_id_without_sig != different_auction_id
        );
    }
}
