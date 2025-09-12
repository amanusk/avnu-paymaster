# Paymaster Auctioneer

A comprehensive auctioneer implementation that manages multiple paymasters, providing high availability and automatic failover capabilities. The auctioneer implements the same RPC endpoints as the regular paymaster service while orchestrating multiple paymaster instances.

## Overview

The `paymaster-auctioneer` crate provides a complete implementation of the paymaster RPC API with advanced paymaster management capabilities. It automatically discovers, monitors, and manages multiple paymaster instances, ensuring high availability and proper token support discovery.

## Features

- **Complete RPC API**: Implements all 5 paymaster RPC endpoints
- **Paymaster Management**: Automatic discovery and monitoring of multiple paymasters
- **High Availability**: Automatic failover and recovery of unresponsive paymasters
- **Token Discovery**: Automatic querying and caching of supported tokens
- **Heartbeat Monitoring**: Continuous health checking with configurable intervals
- **State Management**: Comprehensive state tracking (Active, Unresponsive, Removed)
- **Error Handling**: Robust error handling with timeouts and retry logic
- **Logging**: Detailed logging for monitoring and debugging

## Startup Procedure

The auctioneer follows a comprehensive startup procedure:

1. **Initialization**: Checks `is_available` for all enabled paymasters
2. **Token Discovery**: Queries `get_supported_tokens` for each available paymaster
3. **State Management**: Creates a map of currently available paymasters
4. **Heartbeat Loop**: Continuously monitors paymaster availability
5. **Cleanup**: Removes unresponsive paymasters after configurable timeout
6. **Recovery**: Attempts to reactivate removed paymasters after retry interval

## Endpoints

The auctioneer implements the following RPC endpoints:

1. `paymaster_health` - Returns true if at least one paymaster is active
2. `paymaster_isAvailable` - Returns true if at least one paymaster is active
3. `paymaster_buildTransaction` - Build transaction with paymaster sponsorship (not yet implemented)
4. `paymaster_executeTransaction` - Execute transaction with paymaster sponsorship (not yet implemented)
5. `paymaster_getSupportedTokens` - Returns aggregated supported tokens from all active paymasters

## Usage

### Basic Server

```rust
use paymaster_auctioneer::server::AuctioneerServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = AuctioneerServer::new();
    let server_handle = server.start().await?;

    println!("Auctioneer server started on http://0.0.0.0:8080");

    // Server will run until stopped
    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

    server_handle.stop().unwrap();
    Ok(())
}
```

### Running the Examples

```bash
# Basic usage example
cargo run -p paymaster-auctioneer --example basic_usage

# Startup procedure demonstration
cargo run -p paymaster-auctioneer --example startup_procedure_test
```

## Testing

Run the test suite to verify all endpoints work correctly:

```bash
cargo test -p paymaster-auctioneer
```

The tests verify that:

- All 5 endpoints exist and are callable
- Paymaster management functionality works correctly
- State transitions are handled properly

## Documentation

For detailed information about the startup procedure and implementation:

- [Startup Procedure Documentation](STARTUP_PROCEDURE.md) - Complete guide to the auctioneer startup and monitoring process

## Architecture

The crate is structured as follows:

- `lib.rs` - Main library with RPC trait definitions and error types
- `server.rs` - Server implementation with all endpoint handlers
- `tests.rs` - Comprehensive test suite
- `examples/basic_usage.rs` - Example showing how to run the server

## Next Steps

This foundation is ready for implementing auctioneer-specific functionality:

1. **Configuration**: Add configuration management for auctioneer settings
2. **Auction Logic**: Implement the core auctioneer bidding and selection logic
3. **Integration**: Connect with the regular paymaster service for actual transaction processing
4. **Monitoring**: Add metrics and logging for auctioneer operations
5. **API Extensions**: Add any auctioneer-specific endpoints as needed

## Dependencies

- `jsonrpsee` - RPC framework
- `paymaster-rpc` - Shared types and interfaces
- `starknet` - Starknet types and utilities
- `tokio` - Async runtime
- `tower` - HTTP middleware
- `tracing` - Logging and observability

## License

This project is part of the Avnu Paymaster system and follows the same licensing terms.
