use starknet::core::types::Felt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{info, instrument, warn};

use crate::auction::AuctionManager;
use crate::paymaster_manager::PaymasterManager;
use crate::{AuctionResult, AuctioneerConfig, Error, ExecuteRequest, ExecuteResponse};

/// Execute transaction endpoint implementation
pub struct ExecuteTransactionEndpoint {
    pub auction_manager: Arc<AuctionManager>,
    pub auction_results: Arc<RwLock<HashMap<Felt, AuctionResult>>>,
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
    pub config: AuctioneerConfig,
}

impl ExecuteTransactionEndpoint {
    pub fn new(
        auction_manager: Arc<AuctionManager>,
        auction_results: Arc<RwLock<HashMap<Felt, AuctionResult>>>,
        paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
        config: AuctioneerConfig,
    ) -> Self {
        Self {
            auction_manager,
            auction_results,
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
        let auction_result = {
            let results = self.auction_results.read().await;
            results.get(&auction_id).cloned()
        };

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
        {
            let mut results = self.auction_results.write().await;
            if let Some(removed) = results.remove(&auction_id) {
                info!("Immediately cleared executed auction: {} (created at: {:?})", auction_id, removed.created_at);
            }
        }

        Ok(response)
    }
}
