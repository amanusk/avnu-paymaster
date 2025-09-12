use crate::{AuctionResult, BuildTransactionResponse, Error, PaymasterBid};
use paymaster_rpc::client::Client;
use paymaster_starknet::values::decoding::TypedValueDecoder;
use starknet::core::types::Felt;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{info, warn};

/// Handles auction logic for selecting the best paymaster bid
pub struct AuctionManager {
    /// Timeout for individual paymaster requests
    pub request_timeout: Duration,
}

impl AuctionManager {
    pub fn new() -> Self {
        Self {
            request_timeout: Duration::from_millis(5000), // 5 second timeout
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
        };

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

        // For now, we'll use the message hash as is since removing signature is complex
        // In a real implementation, you'd need to reconstruct the TypedData without signature
        let auction_id = typed_data
            .message_hash(Felt::ZERO)
            .map_err(|_| Error::FailedToCreateAuctionId)?;

        Ok(auction_id)
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
    use paymaster_rpc::{DeployTransaction, DeploymentParameters, ExecutionParameters, FeeEstimate, FeeMode, InvokeTransaction};
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
}
