# Auctioneer Startup Procedure

This document describes the complete startup procedure implemented in the auctioneer system.

## Overview

The auctioneer follows a comprehensive startup and monitoring procedure to manage multiple paymasters, ensuring high availability and proper token support discovery.

## Startup Sequence

### 1. Initialization Phase

Upon startup, the auctioneer performs the following steps:

1. **Configuration Loading**: Loads the auctioneer configuration from file or programmatic setup
2. **Paymaster Discovery**: Iterates through all configured paymasters marked as `enabled: true`
3. **Availability Check**: For each enabled paymaster, calls the `is_available` endpoint
4. **Token Discovery**: For available paymasters, queries the `get_supported_tokens` endpoint
5. **State Management**: Creates a map of currently available paymasters with their supported tokens

### 2. Paymaster State Management

The system maintains three states for each paymaster:

- **Active**: Paymaster is responding and available
- **Unresponsive**: Paymaster is not responding but not yet removed
- **Removed**: Paymaster has been removed and can be re-queried after retry interval

### 3. Heartbeat Monitoring

After initialization, the auctioneer starts a background heartbeat loop that:

1. **Regular Checks**: Queries all paymasters via `is_available` endpoint according to `heartbeat_interval_ms`
2. **State Updates**: Updates paymaster states based on availability responses
3. **Cleanup Logic**: Removes paymasters that have been unresponsive for `cleanup_interval_ms`
4. **Retry Logic**: Attempts to reactivate removed paymasters after 10 minutes (configurable)

## Configuration Parameters

The startup procedure uses the following configuration parameters:

- `heartbeat_interval_ms`: How often to check paymaster availability (default: 30 seconds)
- `cleanup_interval_ms`: How long to wait before removing unresponsive paymasters (default: 60 seconds)
- `paymasters`: List of paymaster configurations with `enabled` flag

## Error Handling

The system includes comprehensive error handling:

- **Timeout Protection**: 5-second timeout for availability checks, 10-second timeout for token queries
- **Graceful Degradation**: Continues operation even if some paymasters are unavailable
- **Logging**: Detailed logging at appropriate levels (info, warn, error, debug)
- **Recovery**: Automatic retry and reactivation of previously unavailable paymasters

## API Endpoints

The auctioneer exposes the following endpoints that utilize the paymaster management:

- `paymaster_health`: Returns true if at least one paymaster is active
- `paymaster_isAvailable`: Returns true if at least one paymaster is active
- `paymaster_getSupportedTokens`: Returns aggregated supported tokens from all active paymasters
- `paymaster_buildTransaction`: (Not yet implemented)
- `paymaster_executeTransaction`: (Not yet implemented)

## Monitoring and Logging

The system provides extensive logging:

- **Startup**: Logs configuration details and initialization progress
- **Availability**: Logs paymaster availability status changes
- **Token Discovery**: Logs supported token counts for each paymaster
- **State Changes**: Logs when paymasters are removed or reactivated
- **Errors**: Logs connection errors and timeouts

## Example Usage

```rust
use paymaster_auctioneer::{server::AuctioneerServer, AuctioneerConfig, PaymasterConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = AuctioneerConfig {
        auction_timeout_ms: 5000,
        heartbeat_interval_ms: 30000,
        cleanup_interval_ms: 60000,
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
        ],
    };

    let server = AuctioneerServer::new(config);
    let server_handle = server.start().await?;

    // Server will now:
    // 1. Check availability of both paymasters
    // 2. Query supported tokens for available paymasters
    // 3. Start heartbeat monitoring
    // 4. Handle unresponsive paymasters
    // 5. Retry removed paymasters after 10 minutes

    // Keep server running...
    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

    server_handle.stop().unwrap();
    Ok(())
}
```

## Testing

Run the startup procedure test:

```bash
cargo run -p paymaster-auctioneer --example startup_procedure_test
```

This will demonstrate the complete startup procedure with detailed logging output.

## Architecture

The implementation consists of:

- **PaymasterManager**: Core state management and monitoring logic
- **AuctioneerServer**: RPC server that uses PaymasterManager
- **PaymasterInfo**: Individual paymaster state tracking
- **Client Integration**: Uses paymaster-rpc client for communication

This architecture ensures separation of concerns, testability, and maintainability while providing robust paymaster management capabilities.
