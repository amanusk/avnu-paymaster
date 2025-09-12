use async_trait::async_trait;
use hyper::http::Extensions;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::{RpcServiceBuilder, ServerBuilder, ServerHandle};
use tracing::{info, instrument};

use crate::{AuctioneerAPIServer, BuildTransactionRequest, BuildTransactionResponse, Error, ExecuteRequest, ExecuteResponse, TokenPrice};

pub struct AuctioneerServer {
    // Configuration will be added later
}

impl AuctioneerServer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn start(self) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
        let url = "0.0.0.0:8080"; // Default port, will be configurable later
        info!("Starting Auctioneer RPC server at {}", url);

        let http_middleware = tower::ServiceBuilder::new()
            .layer(tower_http::cors::CorsLayer::permissive())
            .layer(ProxyGetRequestLayer::new("/health", "paymaster_health").unwrap());

        let rpc_middleware = RpcServiceBuilder::new();

        let server = ServerBuilder::default()
            .max_connections(1024)
            .http_only()
            .set_http_middleware(http_middleware)
            .set_rpc_middleware(rpc_middleware)
            .build(url)
            .await?;

        Ok(server.start(self.into_rpc()))
    }
}

#[async_trait]
impl AuctioneerAPIServer for AuctioneerServer {
    #[instrument(name = "paymaster_health", skip(self))]
    async fn health(&self, _: &Extensions) -> Result<bool, Error> {
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_isAvailable", skip(self, _ext))]
    async fn is_available(&self, _ext: &Extensions) -> Result<bool, Error> {
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_buildTransaction", skip(self, _ext, _params))]
    async fn build_transaction(&self, _ext: &Extensions, _params: BuildTransactionRequest) -> Result<BuildTransactionResponse, Error> {
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_executeTransaction", skip(self, _ext, _params))]
    async fn execute_transaction(&self, _ext: &Extensions, _params: ExecuteRequest) -> Result<ExecuteResponse, Error> {
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_getSupportedTokens", skip(self, _ext))]
    async fn get_supported_tokens(&self, _ext: &Extensions) -> Result<Vec<TokenPrice>, Error> {
        Err(Error::NotYetImplemented)
    }
}
