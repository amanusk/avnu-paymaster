use async_trait::async_trait;
use hyper::http::Extensions;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::{RpcServiceBuilder, ServerBuilder, ServerHandle};
use std::fs;
use tracing::{info, instrument};

use crate::{AuctioneerAPIServer, AuctioneerConfig, BuildTransactionRequest, BuildTransactionResponse, Error, ExecuteRequest, ExecuteResponse, TokenPrice};

pub struct AuctioneerServer {
    pub config: AuctioneerConfig,
}

impl AuctioneerServer {
    pub fn new(config: AuctioneerConfig) -> Self {
        Self { config }
    }

    /// Load configuration from a JSON file
    pub fn from_config_file(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_data = fs::read_to_string(path)?;
        let config: AuctioneerConfig = serde_json::from_str(&config_data)?;
        Ok(Self::new(config))
    }

    pub async fn start(self) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("0.0.0.0:{}", self.config.port);
        info!("Starting Auctioneer RPC server at {}", url);
        info!("Chain ID: {}", self.config.chain_id);
        info!("Auction timeout: {}ms", self.config.auction_timeout_ms);
        info!("Heartbeat interval: {}ms", self.config.heartbeat_interval_ms);
        info!("Cleanup interval: {}ms", self.config.cleanup_interval_ms);
        info!("Configured paymasters: {}", self.config.paymasters.len());

        for paymaster in &self.config.paymasters {
            if paymaster.enabled {
                info!("  - {}: {} (enabled)", paymaster.name, paymaster.url);
            } else {
                info!("  - {}: {} (disabled)", paymaster.name, paymaster.url);
            }
        }

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
