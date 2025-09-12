use async_trait::async_trait;
use hyper::http::Extensions;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::{RpcServiceBuilder, ServerBuilder, ServerHandle};
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument};

use crate::auction::AuctionManager;
use crate::paymaster_manager::PaymasterManager;
use crate::{
    AuctionResult, AuctioneerAPIServer, AuctioneerConfig, BuildTransactionRequest, BuildTransactionResponse, Error, ExecuteRequest, ExecuteResponse, TokenPrice,
};
use starknet::core::types::Felt;
use std::collections::HashMap;

pub struct AuctioneerServer {
    pub config: AuctioneerConfig,
    pub paymaster_manager: Arc<RwLock<Option<PaymasterManager>>>,
    pub auction_results: Arc<RwLock<HashMap<Felt, AuctionResult>>>,
    pub auction_manager: AuctionManager,
}

impl AuctioneerServer {
    pub fn new(config: AuctioneerConfig) -> Self {
        Self {
            config,
            paymaster_manager: Arc::new(RwLock::new(None)),
            auction_results: Arc::new(RwLock::new(HashMap::new())),
            auction_manager: AuctionManager::new(),
        }
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

        // Initialize the paymaster manager
        info!("Initializing paymaster manager...");
        let paymaster_manager = PaymasterManager::new(self.config.clone());
        paymaster_manager.initialize().await?;

        // Store the manager in the server
        {
            let mut manager = self.paymaster_manager.write().await;
            *manager = Some(paymaster_manager);
        }

        // Start the heartbeat monitoring in the background
        let manager_arc = self.paymaster_manager.clone();
        tokio::spawn(async move {
            if let Some(manager) = manager_arc.read().await.as_ref() {
                manager.start_heartbeat().await;
            }
        });

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
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            Ok(active_count > 0)
        } else {
            Ok(false)
        }
    }

    #[instrument(name = "paymaster_isAvailable", skip(self, _ext))]
    async fn is_available(&self, _ext: &Extensions) -> Result<bool, Error> {
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            Ok(active_count > 0)
        } else {
            Ok(false)
        }
    }

    #[instrument(name = "paymaster_buildTransaction", skip(self, _ext, params))]
    async fn build_transaction(&self, _ext: &Extensions, params: BuildTransactionRequest) -> Result<BuildTransactionResponse, Error> {
        // Get active paymasters
        let manager = self.paymaster_manager.read().await;
        let paymaster_manager = manager.as_ref().ok_or(Error::NoActivePaymasters)?;
        let active_paymasters = paymaster_manager.get_active_paymasters().await;

        if active_paymasters.is_empty() {
            return Err(Error::NoActivePaymasters);
        }

        // Only process non-sponsored transactions
        if params.parameters.fee_mode().is_sponsored() {
            return Err(Error::NotYetImplemented);
        }

        // Prepare paymaster list for auction
        let paymasters: Vec<(String, String)> = active_paymasters
            .into_iter()
            .map(|(name, info)| (name, info.config.url))
            .collect();

        // Run the auction
        let auction_result = self.auction_manager.run_auction(paymasters, params).await?;

        // Store auction result
        {
            let mut results = self.auction_results.write().await;
            results.insert(auction_result.auction_id, auction_result.clone());
        }

        Ok(auction_result.response)
    }

    #[instrument(name = "paymaster_executeTransaction", skip(self, _ext, _params))]
    async fn execute_transaction(&self, _ext: &Extensions, _params: ExecuteRequest) -> Result<ExecuteResponse, Error> {
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_getSupportedTokens", skip(self, _ext))]
    async fn get_supported_tokens(&self, _ext: &Extensions) -> Result<Vec<TokenPrice>, Error> {
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let tokens = manager.get_all_supported_tokens().await;
            Ok(tokens)
        } else {
            Ok(Vec::new())
        }
    }
}
