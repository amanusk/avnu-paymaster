use paymaster_rpc::{client::Client, TokenPrice};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::{AuctioneerConfig, PaymasterConfig};

/// Represents the state of a paymaster
#[derive(Debug, Clone, PartialEq)]
pub enum PaymasterState {
    /// Paymaster is active and responding
    Active,
    /// Paymaster is not responding and will be removed after cleanup interval
    Unresponsive,
    /// Paymaster has been removed and can be re-queried after retry interval
    Removed,
}

/// Information about a paymaster's current state
#[derive(Debug, Clone)]
pub struct PaymasterInfo {
    pub config: PaymasterConfig,
    pub state: PaymasterState,
    pub supported_tokens: Vec<TokenPrice>,
    pub last_available_check: Instant,
    pub last_removed_at: Option<Instant>,
}

impl PaymasterInfo {
    pub fn new(config: PaymasterConfig) -> Self {
        Self {
            config,
            state: PaymasterState::Active,
            supported_tokens: Vec::new(),
            last_available_check: Instant::now(),
            last_removed_at: None,
        }
    }
}

/// Manages the state of all paymasters for the auctioneer
pub struct PaymasterManager {
    paymasters: Arc<RwLock<HashMap<String, PaymasterInfo>>>,
    config: AuctioneerConfig,
    retry_interval: Duration,
}

impl PaymasterManager {
    pub fn new(config: AuctioneerConfig) -> Self {
        // Use retry_interval_ms from config, defaulting to 10 minutes (600000 ms) if not specified
        let retry_interval_ms = config.retry_interval_ms.unwrap_or(600_000);
        let retry_interval = Duration::from_millis(retry_interval_ms);

        Self {
            paymasters: Arc::new(RwLock::new(HashMap::new())),
            config,
            retry_interval,
        }
    }

    /// Initialize the paymaster manager by checking all enabled paymasters
    pub async fn initialize(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Initializing paymaster manager...");

        let mut active_paymasters = HashMap::new();

        for paymaster_config in &self.config.paymasters {
            if !paymaster_config.enabled {
                info!("Skipping disabled paymaster: {}", paymaster_config.name);
                continue;
            }

            info!("Checking availability of paymaster: {}", paymaster_config.name);

            let mut paymaster_info = PaymasterInfo::new(paymaster_config.clone());

            // Check if paymaster is available
            match self.check_paymaster_availability(&mut paymaster_info).await {
                Ok(true) => {
                    info!("Paymaster {} is available", paymaster_config.name);

                    // Query supported tokens
                    match self.fetch_supported_tokens(&mut paymaster_info).await {
                        Ok(tokens) => {
                            paymaster_info.supported_tokens = tokens;
                            info!("Paymaster {} supports {} tokens", paymaster_config.name, paymaster_info.supported_tokens.len());
                        },
                        Err(e) => {
                            warn!("Failed to fetch supported tokens for {}: {}", paymaster_config.name, e);
                        },
                    }

                    active_paymasters.insert(paymaster_config.name.clone(), paymaster_info);
                },
                Ok(false) => {
                    warn!("Paymaster {} is not available", paymaster_config.name);
                    paymaster_info.state = PaymasterState::Unresponsive;
                    active_paymasters.insert(paymaster_config.name.clone(), paymaster_info);
                },
                Err(e) => {
                    error!("Error checking availability of {}: {}", paymaster_config.name, e);
                    paymaster_info.state = PaymasterState::Unresponsive;
                    active_paymasters.insert(paymaster_config.name.clone(), paymaster_info);
                },
            }
        }

        {
            let mut paymasters = self.paymasters.write().await;
            *paymasters = active_paymasters;
        }

        let active_count = self.count_active_paymasters().await;
        info!("Initialization complete. {} active paymasters", active_count);

        Ok(())
    }

    /// Start the heartbeat monitoring loop
    pub async fn start_heartbeat(&self) {
        let heartbeat_interval = Duration::from_millis(self.config.heartbeat_interval_ms);
        let cleanup_interval = Duration::from_millis(self.config.cleanup_interval_ms);

        info!("Starting heartbeat monitoring with interval: {:?}", heartbeat_interval);

        loop {
            sleep(heartbeat_interval).await;

            debug!("Running heartbeat check...");

            let mut paymasters = self.paymasters.write().await;
            let mut to_remove = Vec::new();
            let mut to_reactivate = Vec::new();

            for (name, paymaster_info) in paymasters.iter_mut() {
                match paymaster_info.state {
                    PaymasterState::Active => {
                        // Check if still available
                        match self.check_paymaster_availability(paymaster_info).await {
                            Ok(true) => {
                                paymaster_info.last_available_check = Instant::now();
                                debug!("Paymaster {} is still available", name);
                            },
                            Ok(false) => {
                                warn!("Paymaster {} is no longer available", name);
                                paymaster_info.state = PaymasterState::Unresponsive;
                                paymaster_info.last_available_check = Instant::now();
                            },
                            Err(e) => {
                                error!("Error checking availability of {}: {}", name, e);
                                paymaster_info.state = PaymasterState::Unresponsive;
                                paymaster_info.last_available_check = Instant::now();
                            },
                        }
                    },
                    PaymasterState::Unresponsive => {
                        // Check if we should remove it
                        if paymaster_info.last_available_check.elapsed() >= cleanup_interval {
                            warn!("Removing unresponsive paymaster: {}", name);
                            paymaster_info.state = PaymasterState::Removed;
                            paymaster_info.last_removed_at = Some(Instant::now());
                            to_remove.push(name.clone());
                        }
                    },
                    PaymasterState::Removed => {
                        // Check if we should try to reactivate it
                        if let Some(removed_at) = paymaster_info.last_removed_at {
                            if removed_at.elapsed() >= self.retry_interval {
                                info!("Attempting to reactivate paymaster: {}", name);
                                to_reactivate.push(name.clone());
                            }
                        }
                    },
                }
            }

            // Remove unresponsive paymasters
            for name in to_remove {
                warn!("Paymaster {} removed from active list", name);
            }

            // Try to reactivate removed paymasters
            for name in to_reactivate {
                if let Some(paymaster_info) = paymasters.get_mut(&name) {
                    match self.check_paymaster_availability(paymaster_info).await {
                        Ok(true) => {
                            info!("Paymaster {} is back online, reactivating", name);
                            paymaster_info.state = PaymasterState::Active;
                            paymaster_info.last_available_check = Instant::now();

                            // Refresh supported tokens
                            if let Ok(tokens) = self.fetch_supported_tokens(paymaster_info).await {
                                paymaster_info.supported_tokens = tokens;
                                info!("Paymaster {} supports {} tokens", name, paymaster_info.supported_tokens.len());
                            }
                        },
                        Ok(false) => {
                            debug!("Paymaster {} is still not available", name);
                        },
                        Err(e) => {
                            error!("Error checking availability of {}: {}", name, e);
                        },
                    }
                }
            }
        }
    }

    /// Check if a paymaster is available
    async fn check_paymaster_availability(&self, paymaster_info: &mut PaymasterInfo) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let timeout = Duration::from_secs(5); // 5 second timeout for availability checks
        let client = Client::new(&paymaster_info.config.url);

        let result = tokio::time::timeout(timeout, client.is_available()).await;

        match result {
            Ok(Ok(available)) => Ok(available),
            Ok(Err(e)) => Err(Box::new(e)),
            Err(_) => Err("Timeout checking paymaster availability".into()),
        }
    }

    /// Fetch supported tokens for a paymaster
    async fn fetch_supported_tokens(&self, paymaster_info: &mut PaymasterInfo) -> Result<Vec<TokenPrice>, Box<dyn std::error::Error + Send + Sync>> {
        let timeout = Duration::from_secs(10); // 10 second timeout for token queries
        let client = Client::new(&paymaster_info.config.url);

        let result = tokio::time::timeout(timeout, client.get_supported_tokens()).await;

        match result {
            Ok(Ok(tokens)) => Ok(tokens),
            Ok(Err(e)) => Err(Box::new(e)),
            Err(_) => Err("Timeout fetching supported tokens".into()),
        }
    }

    /// Get all active paymasters
    pub async fn get_active_paymasters(&self) -> HashMap<String, PaymasterInfo> {
        let paymasters = self.paymasters.read().await;
        paymasters
            .iter()
            .filter(|(_, info)| info.state == PaymasterState::Active)
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect()
    }

    /// Get a specific paymaster by name
    pub async fn get_paymaster(&self, name: &str) -> Option<PaymasterInfo> {
        let paymasters = self.paymasters.read().await;
        paymasters.get(name).cloned()
    }

    /// Count active paymasters
    pub async fn count_active_paymasters(&self) -> usize {
        let paymasters = self.paymasters.read().await;
        paymasters.values().filter(|info| info.state == PaymasterState::Active).count()
    }

    /// Get all supported tokens from all active paymasters
    pub async fn get_all_supported_tokens(&self) -> Vec<TokenPrice> {
        let paymasters = self.paymasters.read().await;
        let mut token_map = std::collections::HashMap::new();

        for paymaster_info in paymasters.values() {
            if paymaster_info.state == PaymasterState::Active {
                for token in &paymaster_info.supported_tokens {
                    // Use token_address as the key to ensure uniqueness
                    token_map.insert(token.token_address, *token);
                }
            }
        }

        // Convert the HashMap values back to a Vec
        token_map.into_values().collect()
    }

    /// Start the supported tokens refresh task that runs every retry_interval
    pub async fn start_token_refresh_task(&self) {
        info!("Starting supported tokens refresh task with interval: {:?}", self.retry_interval);

        loop {
            sleep(self.retry_interval).await;

            debug!("Running supported tokens refresh check...");

            let mut paymasters = self.paymasters.write().await;
            let mut updated_count = 0;

            for (name, paymaster_info) in paymasters.iter_mut() {
                if paymaster_info.state == PaymasterState::Active {
                    match self.fetch_supported_tokens(paymaster_info).await {
                        Ok(tokens) => {
                            let old_count = paymaster_info.supported_tokens.len();
                            paymaster_info.supported_tokens = tokens;
                            let new_count = paymaster_info.supported_tokens.len();

                            if old_count != new_count {
                                info!("Paymaster {} token count changed: {} -> {}", name, old_count, new_count);
                            } else {
                                debug!("Paymaster {} token list refreshed ({} tokens)", name, new_count);
                            }
                            updated_count += 1;
                        },
                        Err(e) => {
                            warn!("Failed to refresh supported tokens for {}: {}", name, e);
                        },
                    }
                }
            }

            info!("Supported tokens refresh completed for {} active paymasters", updated_count);
        }
    }
}
