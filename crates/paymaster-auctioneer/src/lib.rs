use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use serde::Deserialize;
use thiserror::Error;

// Re-export types from paymaster-rpc for convenience
pub use paymaster_rpc::{BuildTransactionRequest, BuildTransactionResponse, ExecuteRequest, ExecuteResponse, TokenPrice};

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

#[derive(Deserialize, Error, Debug)]
pub enum Error {
    #[error("not yet implemented")]
    NotYetImplemented,
}

impl<'a> From<Error> for ErrorObject<'a> {
    fn from(value: Error) -> Self {
        match value {
            Error::NotYetImplemented => ErrorObject::owned(999, "Not yet implemented", Some(value.to_string())),
        }
    }
}

pub mod server;

#[cfg(test)]
mod tests;
