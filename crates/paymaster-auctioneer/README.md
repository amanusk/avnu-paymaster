# Paymaster Auctioneer

A new crate that implements the same RPC endpoints as the regular paymaster service, designed to be implemented as an auctioneer that doesn't require any new endpoints on the paymaster side.

## Overview

The `paymaster-auctioneer` crate provides a clean implementation of the paymaster RPC API that can be extended to implement auctioneer functionality. Currently, all endpoints return "not yet implemented" errors, providing a solid foundation for future development.

## Features

- **Complete RPC API**: Implements all 5 paymaster RPC endpoints
- **Same Interface**: Uses the same types and method signatures as the regular paymaster
- **Test Coverage**: Comprehensive tests verify all endpoints exist and return expected errors
- **Clean Architecture**: Ready for auctioneer-specific implementation

## Endpoints

The auctioneer implements the following RPC endpoints:

1. `paymaster_health` - Health check endpoint
2. `paymaster_isAvailable` - Service availability check
3. `paymaster_buildTransaction` - Build transaction with paymaster sponsorship
4. `paymaster_executeTransaction` - Execute transaction with paymaster sponsorship
5. `paymaster_getSupportedTokens` - Get list of supported tokens

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

### Running the Example

```bash
cargo run -p paymaster-auctioneer --example basic_usage
```

## Testing

Run the test suite to verify all endpoints work correctly:

```bash
cargo test -p paymaster-auctioneer
```

The tests verify that:

- All 5 endpoints exist and are callable
- All endpoints return the expected "not yet implemented" error
- The error handling works correctly

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
