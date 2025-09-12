use paymaster_auctioneer::server::AuctioneerServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Load configuration from file
    let config_path = "auctioneer-config.json";
    let server = AuctioneerServer::from_config_file(config_path)?;
    let server_handle = server.start().await?;

    println!("Auctioneer server started on http://0.0.0.0:8080");
    println!("Configuration loaded from: {}", config_path);
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
