use starknet::core::types::Felt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{info, instrument, warn};

use crate::auction_manager::AuctionManager;
use crate::paymaster_manager::{PaymasterInfo, PaymasterManager};
use crate::{AuctioneerConfig, BuildTransactionRequest, BuildTransactionResponse, Error, ExecuteRequest, ExecuteResponse, TokenPrice};
use paymaster_rpc;

/// Filter paymasters that support the requested gas_token
pub fn filter_paymasters_by_gas_token(paymasters: HashMap<String, PaymasterInfo>, requested_gas_token: Felt) -> Vec<(String, String)> {
    paymasters
        .into_iter()
        .filter_map(|(name, info)| {
            let supports_token = info
                .supported_tokens
                .iter()
                .any(|token| token.token_address == requested_gas_token);

            if supports_token {
                info!("Paymaster {} supports gas_token {}", name, requested_gas_token);
                Some((name, info.config.url))
            } else {
                info!("Paymaster {} does not support gas_token {}, filtering out", name, requested_gas_token);
                None
            }
        })
        .collect()
}

/// Execute transaction endpoint implementation
pub struct ExecuteTransactionEndpoint {
    pub auction_manager: Arc<AuctionManager>,
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
    pub config: AuctioneerConfig,
}

impl ExecuteTransactionEndpoint {
    pub fn new(auction_manager: Arc<AuctionManager>, paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>, config: AuctioneerConfig) -> Self {
        Self {
            auction_manager,
            paymaster_manager,
            config,
        }
    }

    #[instrument(name = "paymaster_executeTransaction", skip(self, params))]
    pub async fn execute_transaction(&self, params: ExecuteRequest) -> Result<ExecuteResponse, Error> {
        info!("Execute transaction endpoint invoked");

        // Extract the typed data from the request
        let typed_data = match &params.transaction {
            paymaster_rpc::ExecutableTransactionParameters::Invoke { invoke } => &invoke.typed_data,
            paymaster_rpc::ExecutableTransactionParameters::DeployAndInvoke { invoke, .. } => &invoke.typed_data,
            paymaster_rpc::ExecutableTransactionParameters::Deploy { .. } => {
                info!("Deploy transactions not supported for execution");
                return Err(Error::NotYetImplemented);
            },
        };

        // Generate auction ID from the typed data (without signature)
        let auction_id = self.auction_manager.generate_auction_id_from_typed_data(typed_data)?;
        info!("Generated auction ID from typed data: {}", auction_id);

        // Check if the auction exists
        let auction_result = self.auction_manager.get_auction_result(auction_id).await;

        let auction_result = match auction_result {
            Some(result) => {
                info!("Found auction result for ID: {}, winner: {}", auction_id, result.winning_paymaster);
                result
            },
            None => {
                info!("No auction found for ID: {}", auction_id);
                return Err(Error::NoAuctionFound);
            },
        };

        // Get the winning paymaster's URL
        let manager = self.paymaster_manager.read().await;
        let paymaster_manager = manager.as_ref().ok_or(Error::ServiceNotAvailable)?;
        let winning_paymaster_info = paymaster_manager
            .get_paymaster(&auction_result.winning_paymaster)
            .await
            .ok_or_else(|| Error::PaymasterRequestFailed(format!("Winning paymaster {} not found", auction_result.winning_paymaster)))?;

        info!(
            "Forwarding execute request to winning paymaster: {} at {}",
            auction_result.winning_paymaster, winning_paymaster_info.config.url
        );

        // Create a client for the winning paymaster
        let client = paymaster_rpc::client::Client::new(&winning_paymaster_info.config.url);

        // Get retry configuration with defaults
        let retry_count = self.config.execute_retry_count.unwrap_or(3);
        let retry_delay_ms = self.config.execute_retry_delay_ms.unwrap_or(1000);

        // Forward the execute request to the winning paymaster with retry logic
        let mut last_error = None;
        let mut response = None;

        for attempt in 0..=retry_count {
            match client.execute_transaction(params.clone()).await {
                Ok(resp) => {
                    response = Some(resp);
                    if attempt > 0 {
                        info!(
                            "Successfully executed transaction with paymaster {} on attempt {}",
                            auction_result.winning_paymaster,
                            attempt + 1
                        );
                    }
                    break;
                },
                Err(e) => {
                    last_error = Some(e);
                    if attempt < retry_count {
                        warn!(
                            "Failed to execute transaction with paymaster {} on attempt {}: {}. Retrying in {}ms...",
                            auction_result.winning_paymaster,
                            attempt + 1,
                            last_error.as_ref().unwrap(),
                            retry_delay_ms
                        );
                        sleep(Duration::from_millis(retry_delay_ms)).await;
                    } else {
                        warn!(
                            "Failed to execute transaction with paymaster {} after {} attempts. Last error: {}",
                            auction_result.winning_paymaster,
                            retry_count + 1,
                            last_error.as_ref().unwrap()
                        );
                    }
                },
            }
        }

        let response = response.ok_or_else(|| {
            Error::PaymasterRequestFailed(format!(
                "Failed to execute transaction with winning paymaster after {} retries: {}",
                retry_count,
                last_error
                    .as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "Unknown error".to_string())
            ))
        })?;

        info!(
            "Successfully executed transaction with paymaster: {}, tx_hash: 0x{:x}, tracking_id: {}",
            auction_result.winning_paymaster, response.transaction_hash, response.tracking_id
        );

        // Immediately clear the auction since it has been successfully executed
        if let Some(removed) = self.auction_manager.remove_auction_result(auction_id).await {
            info!("Immediately cleared executed auction: {} (created at: {:?})", auction_id, removed.created_at);
        }

        Ok(response)
    }
}

/// Health check endpoint implementation
pub struct HealthEndpoint {
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
}

impl HealthEndpoint {
    pub fn new(paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>) -> Self {
        Self { paymaster_manager }
    }

    #[instrument(name = "paymaster_health", skip(self))]
    pub async fn health(&self) -> Result<bool, Error> {
        info!("Health check endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            info!("Health check: {} active paymasters", active_count);
            if active_count == 0 {
                return Err(Error::ServiceNotAvailable);
            }
            Ok(true)
        } else {
            info!("Health check: No paymaster manager available");
            Err(Error::ServiceNotAvailable)
        }
    }
}

/// Is available endpoint implementation
pub struct IsAvailableEndpoint {
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
}

impl IsAvailableEndpoint {
    pub fn new(paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>) -> Self {
        Self { paymaster_manager }
    }

    #[instrument(name = "paymaster_isAvailable", skip(self))]
    pub async fn is_available(&self) -> Result<bool, Error> {
        info!("Is available endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            info!("Is available check: {} active paymasters", active_count);
            if active_count == 0 {
                return Err(Error::ServiceNotAvailable);
            }
            Ok(true)
        } else {
            info!("Is available check: No paymaster manager available");
            Err(Error::ServiceNotAvailable)
        }
    }
}

/// Get supported tokens endpoint implementation
pub struct GetSupportedTokensEndpoint {
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
}

impl GetSupportedTokensEndpoint {
    pub fn new(paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>) -> Self {
        Self { paymaster_manager }
    }

    #[instrument(name = "paymaster_getSupportedTokens", skip(self))]
    pub async fn get_supported_tokens(&self) -> Result<Vec<TokenPrice>, Error> {
        info!("Get supported tokens endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            if active_count == 0 {
                info!("No active paymasters available, returning error");
                return Err(Error::ServiceNotAvailable);
            }
            let tokens = manager.get_all_supported_tokens().await;
            info!("Retrieved {} unique supported tokens (deduplicated by token address)", tokens.len());
            Ok(tokens)
        } else {
            info!("No paymaster manager available, returning error");
            Err(Error::ServiceNotAvailable)
        }
    }
}

/// Build transaction endpoint implementation
pub struct BuildTransactionEndpoint {
    pub auction_manager: Arc<AuctionManager>,
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
}

impl BuildTransactionEndpoint {
    pub fn new(auction_manager: Arc<AuctionManager>, paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>) -> Self {
        Self {
            auction_manager,
            paymaster_manager,
        }
    }

    #[instrument(name = "paymaster_buildTransaction", skip(self, params))]
    pub async fn build_transaction(&self, params: BuildTransactionRequest) -> Result<BuildTransactionResponse, Error> {
        info!("Build transaction endpoint invoked");
        info!("Transaction details: fee_mode={:?}", params.parameters.fee_mode());

        // Log transaction-specific details
        match &params.transaction {
            paymaster_rpc::TransactionParameters::Invoke { invoke } => {
                info!("Invoke transaction: user_address={:?}, calls_count={}", invoke.user_address, invoke.calls.len());
            },
            paymaster_rpc::TransactionParameters::Deploy { deployment } => {
                info!("Deploy transaction: address={:?}, class_hash={:?}", deployment.address, deployment.class_hash);
            },
            paymaster_rpc::TransactionParameters::DeployAndInvoke { deployment, invoke } => {
                info!(
                    "DeployAndInvoke transaction: address={:?}, user_address={:?}, calls_count={}",
                    deployment.address,
                    invoke.user_address,
                    invoke.calls.len()
                );
            },
        }

        // Get active paymasters
        let manager = self.paymaster_manager.read().await;
        let paymaster_manager = manager.as_ref().ok_or(Error::ServiceNotAvailable)?;
        let active_paymasters = paymaster_manager.get_active_paymasters().await;

        info!("Found {} active paymasters for auction", active_paymasters.len());

        if active_paymasters.is_empty() {
            info!("No active paymasters available, returning error");
            return Err(Error::ServiceNotAvailable);
        }

        // Only process non-sponsored transactions
        if params.parameters.fee_mode().is_sponsored() {
            info!("Sponsored transaction detected, not yet implemented");
            return Err(Error::NotYetImplemented);
        }

        // Extract gas_token from the request parameters
        let requested_gas_token = params.parameters.gas_token();
        info!("Requested gas_token: {}", requested_gas_token);

        // Filter paymasters that support the requested gas_token
        let paymasters = filter_paymasters_by_gas_token(active_paymasters, requested_gas_token);

        info!("Found {} paymasters that support gas_token {}", paymasters.len(), requested_gas_token);

        if paymasters.is_empty() {
            info!("No paymasters support the requested gas_token: {}", requested_gas_token);
            return Err(Error::TokenNotSupported);
        }

        info!("Starting auction with {} paymasters", paymasters.len());
        for (name, url) in &paymasters {
            info!("  - Paymaster: {} at {}", name, url);
        }

        // Run the auction
        let auction_result = self.auction_manager.run_auction(paymasters, params).await?;

        info!(
            "Auction completed: winner={}, auction_id={:?}, gas_token={:?}, amount={:?}",
            auction_result.winning_paymaster, auction_result.auction_id, auction_result.gas_token, auction_result.amount
        );

        // Auction result is already stored by the auction manager

        info!("Build transaction completed successfully");
        Ok(auction_result.response)
    }
}
