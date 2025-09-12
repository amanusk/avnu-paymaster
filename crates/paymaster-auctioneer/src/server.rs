use async_trait::async_trait;
use futures::future::BoxFuture;
use hyper::http::Extensions;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::server::{RpcServiceBuilder, ServerBuilder, ServerHandle};
use jsonrpsee::types::Request;
use jsonrpsee::MethodResponse;
use std::borrow::Cow;
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

/// Payload formatter middleware that wraps parameters in array format
/// This ensures compatibility with both RPC clients and HTTP raw calls
#[derive(Clone)]
pub struct PayloadFormatter<S> {
    service: S,
}

impl<S> PayloadFormatter<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    fn wrap_parameters<'a>(&self, mut request: Request<'a>) -> Request<'a> {
        let Some(params) = request.params.clone() else {
            return request;
        };

        let payload = params.get();
        // If the request is already in positional form (i.e array) do nothing
        if payload.starts_with("[") && payload.ends_with("]") {
            return request;
        }

        // Otherwise wrap payload into array
        let Ok(payload) = serde_json::value::to_raw_value(&vec![params]) else {
            return request;
        };

        request.params = Some(Cow::Owned(payload));
        request
    }
}

impl<'a, S> RpcServiceT<'a> for PayloadFormatter<S>
where
    S: RpcServiceT<'a> + Send + Sync + Clone + 'static,
{
    type Future = BoxFuture<'a, MethodResponse>;

    fn call(&self, request: Request<'a>) -> Self::Future {
        let service = self.service.clone();
        let request = self.wrap_parameters(request);

        Box::pin(async move { service.call(request).await })
    }
}

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

        let rpc_middleware = RpcServiceBuilder::new().layer_fn(PayloadFormatter::new);

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
        info!("Health check endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            info!("Health check: {} active paymasters", active_count);
            Ok(active_count > 0)
        } else {
            info!("Health check: No paymaster manager available");
            Ok(false)
        }
    }

    #[instrument(name = "paymaster_isAvailable", skip(self, _ext))]
    async fn is_available(&self, _ext: &Extensions) -> Result<bool, Error> {
        info!("Is available endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let active_count = manager.count_active_paymasters().await;
            info!("Is available check: {} active paymasters", active_count);
            Ok(active_count > 0)
        } else {
            info!("Is available check: No paymaster manager available");
            Ok(false)
        }
    }

    #[instrument(name = "paymaster_buildTransaction", skip(self, _ext, params))]
    async fn build_transaction(&self, _ext: &Extensions, params: BuildTransactionRequest) -> Result<BuildTransactionResponse, Error> {
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
        let paymaster_manager = manager.as_ref().ok_or(Error::NoActivePaymasters)?;
        let active_paymasters = paymaster_manager.get_active_paymasters().await;

        info!("Found {} active paymasters for auction", active_paymasters.len());

        if active_paymasters.is_empty() {
            info!("No active paymasters available, returning error");
            return Err(Error::NoActivePaymasters);
        }

        // Only process non-sponsored transactions
        if params.parameters.fee_mode().is_sponsored() {
            info!("Sponsored transaction detected, not yet implemented");
            return Err(Error::NotYetImplemented);
        }

        // Prepare paymaster list for auction
        let paymasters: Vec<(String, String)> = active_paymasters
            .into_iter()
            .map(|(name, info)| (name, info.config.url))
            .collect();

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

        // Store auction result
        {
            let mut results = self.auction_results.write().await;
            results.insert(auction_result.auction_id, auction_result.clone());
        }

        info!("Build transaction completed successfully");
        Ok(auction_result.response)
    }

    #[instrument(name = "paymaster_executeTransaction", skip(self, _ext, _params))]
    async fn execute_transaction(&self, _ext: &Extensions, _params: ExecuteRequest) -> Result<ExecuteResponse, Error> {
        info!("Execute transaction endpoint invoked (not yet implemented)");
        Err(Error::NotYetImplemented)
    }

    #[instrument(name = "paymaster_getSupportedTokens", skip(self, _ext))]
    async fn get_supported_tokens(&self, _ext: &Extensions) -> Result<Vec<TokenPrice>, Error> {
        info!("Get supported tokens endpoint invoked");
        let manager = self.paymaster_manager.read().await;
        if let Some(manager) = manager.as_ref() {
            let tokens = manager.get_all_supported_tokens().await;
            info!("Retrieved {} unique supported tokens (deduplicated by token address)", tokens.len());
            Ok(tokens)
        } else {
            info!("No paymaster manager available, returning empty token list");
            Ok(Vec::new())
        }
    }
}
