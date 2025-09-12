#[cfg(test)]
mod tests {
    use crate::server::AuctioneerServer;
    use crate::{AuctioneerAPIServer, AuctioneerConfig, Error, PaymasterConfig};
    use hyper::http::Extensions;

    fn create_test_config() -> AuctioneerConfig {
        AuctioneerConfig {
            auction_timeout_ms: 5000,
            heartbeat_interval_ms: 30000,
            cleanup_interval_ms: 60000,
            chain_id: "SN_SEPOLIA".to_string(),
            port: 8080,
            log_level: "info".to_string(),
            paymasters: vec![
                PaymasterConfig {
                    name: "test-paymaster".to_string(),
                    url: "http://localhost:8081".to_string(),
                    enabled: true,
                },
            ],
        }
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_not_implemented() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.health(&Extensions::default()).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotYetImplemented));
    }

    #[tokio::test]
    async fn test_is_available_endpoint_returns_not_implemented() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.is_available(&Extensions::default()).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotYetImplemented));
    }

    #[tokio::test]
    async fn test_build_transaction_endpoint_returns_not_implemented() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let params = paymaster_rpc::BuildTransactionRequest {
            transaction: paymaster_rpc::TransactionParameters::Invoke {
                invoke: paymaster_rpc::InvokeParameters {
                    user_address: starknet::core::types::Felt::from_hex("0x123").unwrap(),
                    calls: vec![],
                },
            },
            parameters: paymaster_rpc::ExecutionParameters::V1 {
                fee_mode: paymaster_rpc::FeeMode::Sponsored,
                time_bounds: Some(paymaster_rpc::TimeBounds {
                    execute_after: 0,
                    execute_before: 0,
                }),
            },
        };

        let result = server.build_transaction(&Extensions::default(), params).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, Error::NotYetImplemented));
        }
    }

    #[tokio::test]
    async fn test_execute_transaction_endpoint_returns_not_implemented() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);

        // For now, we'll skip the complex TypedData creation and just test that the endpoint exists
        // by calling it with a simple request that will fail at the server level
        // This test verifies the endpoint exists and returns the expected error
        let result = server
            .execute_transaction(
                &Extensions::default(),
                paymaster_rpc::ExecuteRequest {
                    transaction: paymaster_rpc::ExecutableTransactionParameters::Deploy {
                        deployment: paymaster_rpc::DeploymentParameters {
                            address: starknet::core::types::Felt::from_hex("0x123").unwrap(),
                            class_hash: starknet::core::types::Felt::from_hex("0x456").unwrap(),
                            salt: starknet::core::types::Felt::from_hex("0x789").unwrap(),
                            calldata: vec![],
                            sigdata: None,
                            version: 1,
                        },
                    },
                    parameters: paymaster_rpc::ExecutionParameters::V1 {
                        fee_mode: paymaster_rpc::FeeMode::Sponsored,
                        time_bounds: Some(paymaster_rpc::TimeBounds {
                            execute_after: 0,
                            execute_before: 0,
                        }),
                    },
                },
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotYetImplemented));
    }

    #[tokio::test]
    async fn test_get_supported_tokens_endpoint_returns_not_implemented() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.get_supported_tokens(&Extensions::default()).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotYetImplemented));
    }

    #[tokio::test]
    async fn test_all_endpoints_return_not_implemented_error() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let ext = Extensions::default();

        // Test health endpoint
        let health_result = server.health(&ext).await;
        assert!(health_result.is_err());
        assert!(matches!(health_result.unwrap_err(), Error::NotYetImplemented));

        // Test is_available endpoint
        let available_result = server.is_available(&ext).await;
        assert!(available_result.is_err());
        assert!(matches!(available_result.unwrap_err(), Error::NotYetImplemented));

        // Test build_transaction endpoint
        let build_params = paymaster_rpc::BuildTransactionRequest {
            transaction: paymaster_rpc::TransactionParameters::Invoke {
                invoke: paymaster_rpc::InvokeParameters {
                    user_address: starknet::core::types::Felt::from_hex("0x123").unwrap(),
                    calls: vec![],
                },
            },
            parameters: paymaster_rpc::ExecutionParameters::V1 {
                fee_mode: paymaster_rpc::FeeMode::Sponsored,
                time_bounds: Some(paymaster_rpc::TimeBounds {
                    execute_after: 0,
                    execute_before: 0,
                }),
            },
        };
        let build_result = server.build_transaction(&ext, build_params).await;
        assert!(build_result.is_err());
        if let Err(e) = build_result {
            assert!(matches!(e, Error::NotYetImplemented));
        }

        // Test execute_transaction endpoint
        let execute_params = paymaster_rpc::ExecuteRequest {
            transaction: paymaster_rpc::ExecutableTransactionParameters::Deploy {
                deployment: paymaster_rpc::DeploymentParameters {
                    address: starknet::core::types::Felt::from_hex("0x123").unwrap(),
                    class_hash: starknet::core::types::Felt::from_hex("0x456").unwrap(),
                    salt: starknet::core::types::Felt::from_hex("0x789").unwrap(),
                    calldata: vec![],
                    sigdata: None,
                    version: 1,
                },
            },
            parameters: paymaster_rpc::ExecutionParameters::V1 {
                fee_mode: paymaster_rpc::FeeMode::Sponsored,
                time_bounds: Some(paymaster_rpc::TimeBounds {
                    execute_after: 0,
                    execute_before: 0,
                }),
            },
        };
        let execute_result = server.execute_transaction(&ext, execute_params).await;
        assert!(execute_result.is_err());
        if let Err(e) = execute_result {
            assert!(matches!(e, Error::NotYetImplemented));
        }

        // Test get_supported_tokens endpoint
        let tokens_result = server.get_supported_tokens(&ext).await;
        assert!(tokens_result.is_err());
        assert!(matches!(tokens_result.unwrap_err(), Error::NotYetImplemented));
    }
}
