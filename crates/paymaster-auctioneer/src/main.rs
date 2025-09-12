use clap::Parser;
use paymaster_auctioneer::server::AuctioneerServer;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "auctioneer")]
#[command(about = "AVNU Paymaster Auctioneer Server")]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "auctioneer-config.json")]
    config: PathBuf,

    /// Log level (trace, debug, info, warn, error) - overrides config file
    #[arg(short, long)]
    log_level: Option<String>,

    /// Port to run the server on - overrides config file
    #[arg(short, long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    // Load configuration from file first
    let mut config = AuctioneerServer::from_config_file(&cli.config.to_string_lossy())?;

    // Override config values with CLI arguments if provided
    if let Some(log_level) = cli.log_level {
        config.config.log_level = log_level;
    }
    if let Some(port) = cli.port {
        config.config.port = port;
    }

    // Initialize logging with the final log level
    let log_level = match config.config.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    info!("Starting AVNU Paymaster Auctioneer Server");
    info!("Configuration file: {}", cli.config.display());
    info!("Port: {}", config.config.port);
    info!("Log level: {}", config.config.log_level);

    // Start the server
    let server_handle = config.start().await?;

    info!("Auctioneer server started successfully!");
    info!("Available endpoints:");
    info!("  - POST /health -> paymaster_health");
    info!("  - POST / -> paymaster_isAvailable");
    info!("  - POST / -> paymaster_buildTransaction");
    info!("  - POST / -> paymaster_executeTransaction");
    info!("  - POST / -> paymaster_getSupportedTokens");
    info!("Press Ctrl+C to stop the server.");

    // Wait for the server to be stopped
    tokio::signal::ctrl_c().await?;
    info!("Shutting down auctioneer server...");

    server_handle.stop().unwrap();
    info!("Auctioneer server stopped.");

    Ok(())
}
