use paymaster_auctioneer::{server::AuctioneerServer, AuctioneerConfig, PaymasterConfig};
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    // Create configuration with test paymasters
    let config = AuctioneerConfig {
        auction_timeout_ms: 5000,
        heartbeat_interval_ms: 10000, // 10 seconds for testing
        cleanup_interval_ms: 20000,   // 20 seconds for testing
        chain_id: "SN_SEPOLIA".to_string(),
        port: 8080,
        log_level: "info".to_string(),
        paymasters: vec![
            PaymasterConfig {
                name: "paymaster-1".to_string(),
                url: "http://localhost:12777".to_string(),
                enabled: true,
            },
            PaymasterConfig {
                name: "paymaster-2".to_string(),
                url: "http://localhost:12778".to_string(),
                enabled: true,
            },
            PaymasterConfig {
                name: "paymaster-3".to_string(),
                url: "http://localhost:12779".to_string(),
                enabled: false, // This one should be skipped
            },
        ],
    };

    info!("Starting Auctioneer with startup procedure test...");
    info!("This will demonstrate the complete startup procedure:");
    info!("1. Check is_available for all enabled paymasters");
    info!("2. Query supported tokens for each available paymaster");
    info!("3. Start heartbeat monitoring loop");
    info!("4. Handle unresponsive paymasters");
    info!("5. Re-query removed paymasters after retry interval");
    info!();

    // Create and start the auctioneer server
    let server = AuctioneerServer::new(config);
    let server_handle = server.start().await?;

    info!("Auctioneer server started successfully!");
    info!("Available endpoints:");
    info!("  - POST /health -> paymaster_health");
    info!("  - POST / -> paymaster_isAvailable");
    info!("  - POST / -> paymaster_buildTransaction");
    info!("  - POST / -> paymaster_executeTransaction");
    info!("  - POST / -> paymaster_getSupportedTokens");
    info!();
    info!("The auctioneer will now:");
    info!("- Monitor paymaster availability every 10 seconds");
    info!("- Remove unresponsive paymasters after 20 seconds");
    info!("- Attempt to reactivate removed paymasters after 10 minutes");
    info!();
    info!("Press Ctrl+C to stop the server.");

    // Wait for the server to be stopped
    // In a real implementation, you'd handle shutdown signals
    tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // 5 minutes for testing

    server_handle.stop().unwrap();
    info!("Auctioneer server stopped.");
    Ok(())
}
