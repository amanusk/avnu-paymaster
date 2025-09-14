use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use serde::{Deserialize, Serialize};
use starknet::core::types::Felt;
use thiserror::Error;

// Re-export types from paymaster-rpc for convenience
pub use paymaster_rpc::{BuildTransactionRequest, BuildTransactionResponse, ExecuteRequest, ExecuteResponse, TokenPrice};

/// Configuration for a single paymaster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymasterConfig {
    /// Name of the paymaster
    pub name: String,
    /// URL endpoint of the paymaster
    pub url: String,
    /// Whether this paymaster is enabled
    pub enabled: bool,
}

/// Main configuration for the auctioneer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuctioneerConfig {
    /// Time for the auction to complete in milliseconds
    pub auction_timeout_ms: u64,
    /// Time to check liveness of connected paymasters in milliseconds
    pub heartbeat_interval_ms: u64,
    /// Time after which a non-responsive paymaster is removed in milliseconds
    pub cleanup_interval_ms: u64,
    /// Starknet chain ID
    pub chain_id: String,
    /// Port to run the server on
    pub port: u16,
    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,
    /// List of paymasters the auctioneer is trying to connect to
    pub paymasters: Vec<PaymasterConfig>,
}

#[rpc(server, client)]
pub trait AuctioneerAPI {
    #[method(name = "paymaster_health", with_extensions)]
    async fn health(&self) -> Result<bool, Error>;

    #[method(name = "paymaster_isAvailable", with_extensions)]
    async fn is_available(&self) -> Result<bool, Error>;

    #[method(name = "paymaster_buildTransaction", with_extensions)]
    async fn build_transaction(&self, params: BuildTransactionRequest) -> Result<BuildTransactionResponse, Error>;

    #[method(name = "paymaster_executeTransaction", with_extensions)]
    async fn execute_transaction(&self, params: ExecuteRequest) -> Result<ExecuteResponse, Error>;

    #[method(name = "paymaster_getSupportedTokens", with_extensions)]
    async fn get_supported_tokens(&self) -> Result<Vec<TokenPrice>, Error>;
}

/// Represents the result of an auction
#[derive(Clone, Serialize, Deserialize)]
pub struct AuctionResult {
    pub auction_id: Felt,
    pub winning_paymaster: String,
    pub gas_token: Felt,
    pub amount: Felt,
    pub response: BuildTransactionResponse,
}

/// Represents a bid from a paymaster
#[derive(Clone)]
pub struct PaymasterBid {
    pub paymaster_name: String,
    pub response: BuildTransactionResponse,
    pub gas_token: Felt,
    pub amount: Felt,
}

#[derive(Deserialize, Error, Debug)]
pub enum Error {
    #[error("not yet implemented")]
    NotYetImplemented,
    #[error("no active paymasters available")]
    NoActivePaymasters,
    #[error("failed to extract gas token information from paymaster response")]
    FailedToExtractGasToken,
    #[error("failed to create auction ID from typed data")]
    FailedToCreateAuctionId,
    #[error("no valid bids received from paymasters")]
    NoValidBids,
    #[error("paymaster request failed: {0}")]
    PaymasterRequestFailed(String),
    #[error("no auction found for the provided transaction")]
    NoAuctionFound,
}

impl<'a> From<Error> for ErrorObject<'a> {
    fn from(value: Error) -> Self {
        match value {
            Error::NotYetImplemented => ErrorObject::owned(999, "Not yet implemented", Some(value.to_string())),
            Error::NoActivePaymasters => ErrorObject::owned(1001, "No active paymasters", Some(value.to_string())),
            Error::FailedToExtractGasToken => ErrorObject::owned(1002, "Failed to extract gas token", Some(value.to_string())),
            Error::FailedToCreateAuctionId => ErrorObject::owned(1003, "Failed to create auction ID", Some(value.to_string())),
            Error::NoValidBids => ErrorObject::owned(1004, "No valid bids", Some(value.to_string())),
            Error::PaymasterRequestFailed(msg) => ErrorObject::owned(1005, "Paymaster request failed", Some(msg)),
            Error::NoAuctionFound => ErrorObject::owned(1006, "No auction found", Some(value.to_string())),
        }
    }
}

pub mod auction;
pub mod paymaster_manager;
pub mod server;

#[cfg(test)]
mod tests;
