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
            paymasters: vec![PaymasterConfig {
                name: "test-paymaster".to_string(),
                url: "http://localhost:8081".to_string(),
                enabled: true,
            }],
        }
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_false_when_no_paymasters() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.health(&Extensions::default()).await;

        // Should return false when no paymaster manager is initialized
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_is_available_endpoint_returns_false_when_no_paymasters() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.is_available(&Extensions::default()).await;

        // Should return false when no paymaster manager is initialized
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_build_transaction_endpoint_returns_no_active_paymasters() {
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

        // Should return error when no paymaster manager is initialized
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, Error::NoActivePaymasters));
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
    async fn test_get_supported_tokens_endpoint_returns_empty_when_no_paymasters() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let result = server.get_supported_tokens(&Extensions::default()).await;

        // Should return empty vector when no paymaster manager is initialized
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_build_transaction_rpc_parameter_parsing() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let ext = Extensions::default();

        // Test with the exact JSON structure from the JavaScript client
        let user_address = starknet::core::types::Felt::from_hex("0x07e85364e14700da309337e9119be2da250859c0e52f4507a8e924972b469fd6").unwrap();
        let token_address = starknet::core::types::Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap();
        let selector = starknet::core::types::Felt::from_hex("0x219209e083275171774dab1df80982e9df2096516f06319c5c6d71ae0a8480c").unwrap();

        let build_params = paymaster_rpc::BuildTransactionRequest {
            transaction: paymaster_rpc::TransactionParameters::Invoke {
                invoke: paymaster_rpc::InvokeParameters {
                    user_address,
                    calls: vec![starknet::core::types::Call {
                        to: token_address,
                        selector,
                        calldata: vec![
                            user_address,
                            starknet::core::types::Felt::from_hex("0x1").unwrap(),
                            starknet::core::types::Felt::from_hex("0x0").unwrap(),
                        ],
                    }],
                },
            },
            parameters: paymaster_rpc::ExecutionParameters::V1 {
                fee_mode: paymaster_rpc::FeeMode::Default { gas_token: token_address },
                time_bounds: None,
            },
        };

        // Test that the parameters are correctly parsed and the method can be called
        // (even though it will fail due to no active paymasters)
        let result = server.build_transaction(&ext, build_params).await;

        // Should return error when no paymaster manager is initialized
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, Error::NoActivePaymasters));
        }
    }

    #[tokio::test]
    async fn test_build_transaction_json_deserialization() {
        // Test that the JSON structure from the JavaScript client can be deserialized
        let json_str = r#"{
            "transaction": {
                "type": "invoke",
                "invoke": {
                    "user_address": "0x07e85364e14700da309337e9119be2da250859c0e52f4507a8e924972b469fd6",
                    "calls": [
                        {
                            "to": "0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7",
                            "selector": "0x219209e083275171774dab1df80982e9df2096516f06319c5c6d71ae0a8480c",
                            "calldata": [
                                "0x7e85364e14700da309337e9119be2da250859c0e52f4507a8e924972b469fd6",
                                "0x1",
                                "0x0"
                            ]
                        }
                    ]
                }
            },
            "parameters": {
                "version": "0x1",
                "fee_mode": {
                    "mode": "default",
                    "gas_token": "0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7"
                }
            }
        }"#;

        // Test deserialization
        let result: Result<paymaster_rpc::BuildTransactionRequest, serde_json::Error> = serde_json::from_str(json_str);

        assert!(result.is_ok(), "Failed to deserialize JSON: {:?}", result.err());

        let request = result.unwrap();

        // Verify the deserialized structure
        match request.transaction {
            paymaster_rpc::TransactionParameters::Invoke { invoke } => {
                assert_eq!(
                    invoke.user_address,
                    starknet::core::types::Felt::from_hex("0x07e85364e14700da309337e9119be2da250859c0e52f4507a8e924972b469fd6").unwrap()
                );
                assert_eq!(invoke.calls.len(), 1);

                let call = &invoke.calls[0];
                assert_eq!(
                    call.to,
                    starknet::core::types::Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap()
                );
                assert_eq!(
                    call.selector,
                    starknet::core::types::Felt::from_hex("0x219209e083275171774dab1df80982e9df2096516f06319c5c6d71ae0a8480c").unwrap()
                );
                assert_eq!(call.calldata.len(), 3);
                assert_eq!(
                    call.calldata[0],
                    starknet::core::types::Felt::from_hex("0x7e85364e14700da309337e9119be2da250859c0e52f4507a8e924972b469fd6").unwrap()
                );
                assert_eq!(call.calldata[1], starknet::core::types::Felt::from_hex("0x1").unwrap());
                assert_eq!(call.calldata[2], starknet::core::types::Felt::from_hex("0x0").unwrap());
            },
            _ => panic!("Expected Invoke transaction type"),
        }

        match request.parameters {
            paymaster_rpc::ExecutionParameters::V1 { fee_mode, time_bounds } => {
                match fee_mode {
                    paymaster_rpc::FeeMode::Default { gas_token } => {
                        assert_eq!(
                            gas_token,
                            starknet::core::types::Felt::from_hex("0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7").unwrap()
                        );
                    },
                    _ => panic!("Expected Default fee mode"),
                }
                assert!(time_bounds.is_none());
            },
        }
    }

    #[tokio::test]
    async fn test_all_endpoints_behavior() {
        let config = create_test_config();
        let server = AuctioneerServer::new(config);
        let ext = Extensions::default();

        // Test health endpoint - should return false when no paymaster manager
        let health_result = server.health(&ext).await;
        assert!(health_result.is_ok());
        assert_eq!(health_result.unwrap(), false);

        // Test is_available endpoint - should return false when no paymaster manager
        let available_result = server.is_available(&ext).await;
        assert!(available_result.is_ok());
        assert_eq!(available_result.unwrap(), false);

        // Test build_transaction endpoint - should return error when no paymaster manager is initialized
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
            assert!(matches!(e, Error::NoActivePaymasters));
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

        // Test get_supported_tokens endpoint - should return empty vector when no paymaster manager
        let tokens_result = server.get_supported_tokens(&ext).await;
        assert!(tokens_result.is_ok());
        assert!(tokens_result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_execute_transaction_no_auction_found() {
        use starknet::core::types::{Felt, TypedData};

        let config = AuctioneerConfig {
            auction_timeout_ms: 5000,
            heartbeat_interval_ms: 10000,
            cleanup_interval_ms: 30000,
            chain_id: "SN_SEPOLIA".to_string(),
            port: 8080,
            log_level: "info".to_string(),
            paymasters: vec![],
        };

        let server = AuctioneerServer::new(config);
        let ext = Extensions::default();

        // Create a simple TypedData using JSON deserialization
        let test_json = serde_json::json!({
            "types": {
                "StarknetDomain": [
                    {"name": "name", "type": "shortstring"},
                    {"name": "version", "type": "shortstring"},
                    {"name": "chainId", "type": "shortstring"},
                    {"name": "revision", "type": "shortstring"}
                ],
                "TestMessage": [
                    {"name": "test", "type": "felt"}
                ]
            },
            "primaryType": "TestMessage",
            "domain": {
                "name": "Test",
                "version": "1",
                "chainId": "SN_SEPOLIA",
                "revision": "1"
            },
            "message": {
                "test": "0x123"
            }
        });

        let typed_data: TypedData = serde_json::from_value(test_json).unwrap();

        // Test execute_transaction with Invoke transaction - should return no auction found
        let execute_params = paymaster_rpc::ExecuteRequest {
            transaction: paymaster_rpc::ExecutableTransactionParameters::Invoke {
                invoke: paymaster_rpc::ExecutableInvokeParameters {
                    user_address: Felt::from_hex("0x123").unwrap(),
                    typed_data,
                    signature: vec![],
                },
            },
            parameters: paymaster_rpc::ExecutionParameters::V1 {
                fee_mode: paymaster_rpc::FeeMode::Default {
                    gas_token: Felt::from_hex("0x789").unwrap(),
                },
                time_bounds: None,
            },
        };
        let execute_result = server.execute_transaction(&ext, execute_params).await;
        assert!(execute_result.is_err());
        if let Err(e) = execute_result {
            assert!(matches!(e, Error::NoAuctionFound));
        }
    }
}
