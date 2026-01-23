use alloy::primitives::{Address, Bytes, FixedBytes, U256};
use alloy::rpc::types::BlockId;
use alloy::transports::TransportErrorKind;
use alloy_json_rpc::RpcError;
use alloy_rpc_client::RpcClient;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::HashMap;
use thiserror::Error;

/// Represents a single transaction in the eth_callMany batch
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Transaction {
    /// The address the transaction is sent from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Address>,
    /// The address the transaction is directed to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Address>,
    /// Integer of the gas provided for the transaction execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas: Option<U256>,
    /// Integer of the gas price used for each paid gas
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "gasPrice")]
    pub gas_price: Option<U256>,
    /// Integer of the value sent with this transaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<U256>,
    /// Hash of the method signature and encoded parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Bytes>,
}

/// Block override options for customizing block header properties
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlockOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "blockNumber")]
    pub block_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "blockHash")]
    pub block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coinbase: Option<Address>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<U256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "gasLimit")]
    pub gas_limit: Option<U256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "baseFee")]
    pub base_fee: Option<U256>,
}

/// Represents a bundle of transactions to be executed together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Array of transactions
    pub transactions: Vec<Transaction>,
    /// Block override for this bundle
    #[serde(rename = "blockOverride")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_override: Option<BlockOverride>,
}

/// State overrides for specific accounts (user-facing API with FixedBytes<32>)
#[derive(Debug, Clone, Default)]
pub struct StateOverride {
    /// Balance override
    pub balance: Option<U256>,
    /// Nonce override
    pub nonce: Option<u64>,
    /// Code override
    pub code: Option<Bytes>,
    /// State override (mapping of storage slots to values as 32-byte values)
    pub state: Option<HashMap<FixedBytes<32>, FixedBytes<32>>>,
    /// State diff (alternative to full state override)
    pub state_diff: Option<HashMap<FixedBytes<32>, FixedBytes<32>>>,
}

/// Internal struct for JSON-RPC serialization
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StateOverrideInternal {
    #[serde(skip_serializing_if = "Option::is_none")]
    balance: Option<U256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<Bytes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stateDiff")]
    state_diff: Option<HashMap<String, String>>,
}

impl StateOverride {
    /// Convert to internal representation for JSON-RPC
    fn to_internal(&self) -> StateOverrideInternal {
        StateOverrideInternal {
            balance: self.balance,
            nonce: self.nonce,
            code: self.code.clone(),
            state: self.state.as_ref().map(|map| {
                map.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            }),
            state_diff: self.state_diff.as_ref().map(|map| {
                map.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            }),
        }
    }
}

/// Response from a single transaction in the batch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransactionResponse {
    /// Successful transaction with return value
    Success {
        /// The return value of the transaction, hex encoded
        value: String,
    },
    /// Failed transaction with error message
    Error {
        /// Error message if the transaction failed
        error: String,
    },
}

/// Simulation context specifying where to execute the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationContext {
    /// Block number, tag (e.g., "latest", "safe", "finalized"), or hash
    #[serde(rename = "blockNumber")]
    pub block_number: BlockId,
    /// Transaction index position for simulation initiation
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "transactionIndex")]
    pub transaction_index: Option<u64>,
}

/// Wrapper for making eth_callMany RPC calls
pub struct EthCallMany<'a> {
    client: &'a RpcClient,
}

#[derive(Debug, Error)]
#[error("call many failed")]
pub enum EthCallManyError {
    Serialization(#[from] serde_json::Error),
    Rpc(#[from] RpcError<TransportErrorKind, Box<RawValue>>),
}

impl<'a> EthCallMany<'a> {
    pub fn new(client: &'a RpcClient) -> Self {
        Self { client }
    }

    /// Execute multiple transaction bundles in sequence using eth_callMany RPC method
    ///
    /// # Arguments
    /// * `bundles` - Array of transaction bundles to execute
    /// * `simulation_context` - The block context and transaction index for the simulation
    /// * `state_overrides` - Optional per-address state overrides
    /// * `timeout` - Optional timeout in milliseconds (defaults to 5000ms)
    ///
    /// # Returns
    /// Vec of Vec of TransactionResponse - outer vec is per bundle, inner vec is per transaction
    pub async fn call_many(
        &self,
        bundles: Vec<Bundle>,
        simulation_context: SimulationContext,
        state_overrides: Option<HashMap<Address, StateOverride>>,
        timeout: Option<u64>,
    ) -> Result<Vec<Vec<TransactionResponse>>, EthCallManyError> {
        // Convert state overrides to internal representation with hex strings
        let state_overrides_internal = state_overrides.map(|map| {
            map.into_iter()
                .map(|(addr, override_val)| (addr, override_val.to_internal()))
                .collect::<HashMap<Address, StateOverrideInternal>>()
        });

        let params = vec![
            serde_json::to_value(&bundles)?,
            serde_json::to_value(&simulation_context)?,
            serde_json::to_value(&state_overrides_internal)?,
            serde_json::to_value(&timeout)?,
        ];

        let result: Vec<Vec<TransactionResponse>> =
            self.client.request("eth_callMany", params).await?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use revm::primitives::{address, ruint::aliases::U256};

    use crate::balance_slot::SlotWithAddress;

    use super::*;

    async fn call_usdc_transfer(
        state_overrides: Option<HashMap<Address, StateOverride>>,
    ) -> TransactionResponse {
        use crate::balance_slot::IERC20::transferCall;
        use alloy::primitives::address;
        use alloy::sol_types::SolCall;

        dotenvy::dotenv().ok();
        let rpc_url = std::env::var("ETH_RPC").expect("ETH_RPC not set in .env");

        let user = address!("0x282Cd0c363CCf32629BE74A0A2B1a0Ed6680aE8e");
        let usdc = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let recipient = address!("0x0000000000000000000000000000000000000001");

        let transfer_amount = U256::from(100_000_000u64); // 100 USDC
        let transfer_data = transferCall {
            to: recipient,
            value: transfer_amount,
        }
        .abi_encode()
        .into();

        let bundle = Bundle {
            transactions: vec![Transaction {
                from: Some(user),
                to: Some(usdc),
                data: Some(transfer_data),
                ..Default::default()
            }],
            block_override: None,
        };

        let simulation_context = SimulationContext {
            block_number: BlockId::latest(),
            transaction_index: None,
        };

        let client = alloy_rpc_client::RpcClient::new_http(rpc_url.parse().unwrap());
        let eth_call_many = EthCallMany::new(&client);

        let result = eth_call_many
            .call_many(
                vec![bundle],
                simulation_context,
                state_overrides,
                Some(5000),
            )
            .await;

        let responses = result.expect("eth_callMany RPC call failed");
        responses[0][0].clone()
    }

    #[tokio::test]
    async fn test_set_balance_and_transfer() {
        use std::collections::HashMap;

        let balance_slot = SlotWithAddress {
            address: address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            slot: U256::from_str_radix(
                "54687958836068981284050203780875644944490412624549896910812179654696915778466",
                10,
            )
            .unwrap(),
        };

        let balance_amount = U256::from(1_000_000_000u64); // 1000 USDC

        let mut storage = HashMap::new();
        storage.insert(balance_slot.slot.into(), balance_amount.into());

        let state_override = StateOverride {
            state_diff: Some(storage),
            ..Default::default()
        };

        let mut state_overrides = HashMap::new();
        state_overrides.insert(balance_slot.address, state_override);

        let tx_response = call_usdc_transfer(Some(state_overrides)).await;

        match tx_response {
            TransactionResponse::Success { value } => {
                // ERC20 transfer returns bool (true = 1)
                let expected = "0x0000000000000000000000000000000000000000000000000000000000000001";
                assert_eq!(value, expected, "Transfer should return true");
                println!("Transaction succeeded with return value: {}", value);
            }
            TransactionResponse::Error { error } => {
                panic!("Transaction reverted: {}", error);
            }
        }
    }

    #[tokio::test]
    async fn test_transfer_without_balance_should_revert() {
        let tx_response = call_usdc_transfer(None).await;

        match tx_response {
            TransactionResponse::Success { value } => {
                panic!(
                    "Transaction should have reverted but succeeded with: {}",
                    value
                );
            }
            TransactionResponse::Error { error } => {
                println!("Transaction reverted as expected: {}", error);
                assert!(
                    error.contains("balance") || error.contains("insufficient"),
                    "Error should mention balance/insufficient, got: {}",
                    error
                );
            }
        }
    }
}
