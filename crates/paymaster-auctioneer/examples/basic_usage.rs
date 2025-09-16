use paymaster_auctioneer::{server::AuctioneerServer, AuctioneerConfig, PaymasterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create configuration
    let config = AuctioneerConfig {
        auction_timeout_ms: 5000,
        heartbeat_interval_ms: 30000,
        cleanup_interval_ms: 60000,
        retry_interval_ms: Some(600000),
        execute_retry_count: Some(3),
        execute_retry_delay_ms: Some(1000),
        chain_id: "SN_SEPOLIA".to_string(),
        port: 8080,
        log_level: "info".to_string(),
        paymasters: vec![
            PaymasterConfig {
                name: "paymaster-1".to_string(),
                url: "http://localhost:8081".to_string(),
                enabled: true,
            },
            PaymasterConfig {
                name: "paymaster-2".to_string(),
                url: "http://localhost:8082".to_string(),
                enabled: true,
            },
        ],
    };

    // Create and start the auctioneer server
    let server = AuctioneerServer::new(config);
    let server_handle = server.start().await?;

    println!("Auctioneer server started on http://0.0.0.0:8080");
    println!("Available endpoints:");
    println!("  - POST /health -> paymaster_health");
    println!("  - POST / -> paymaster_isAvailable");
    println!("  - POST / -> paymaster_buildTransaction");
    println!("  - POST / -> paymaster_executeTransaction");
    println!("  - POST / -> paymaster_getSupportedTokens");
    println!();
    println!("All endpoints currently return 'not yet implemented' errors.");
    println!("Press Ctrl+C to stop the server.");

    // Wait for the server to be stopped
    // For now, just sleep to keep the server running
    // In a real implementation, you'd handle shutdown signals
    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

    server_handle.stop().unwrap();
    Ok(())
}
