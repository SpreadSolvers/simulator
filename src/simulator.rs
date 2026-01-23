use crate::{
    balance_slot::FindSlotError,
    eth_call_many::{
        Bundle, EthCallMany, SimulationContext, StateOverride, Transaction, TransactionResponse,
    },
};
use alloy::{
    eips::BlockId,
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall,
    transports::{TransportErrorKind, http::reqwest::Url},
};
use alloy_json_rpc::RpcError;
use revm::{
    Context, ExecuteCommitEvm, ExecuteEvm, MainBuilder, MainContext,
    context::{
        TxEnv,
        result::{EVMError, ExecutionResult, SuccessReason},
    },
    database::{AlloyDB, Cache, CacheDB, DBTransportError, WrapDatabaseAsync},
    primitives::{Address, Bytes, TxKind, U256},
};
use serde_json::value::RawValue;
use std::collections::HashMap;
use thiserror::Error;

use crate::balance_slot::{AlloyCacheDb, IERC20::approveCall, SlotWithAddress, find_balance_slot};

pub struct SimulationParams {
    pub user: Address,
    pub token_in: Address,
    pub amount_in: U256,
    pub to: Address,
    pub calldata: Bytes,
}

pub struct Simulator {
    db_caches: HashMap<u32, Cache>,
}

pub enum TransactionResult {
    Success(Bytes),
    Failed(String),
}

type SimulationResult = Result<Bytes, String>;

pub struct SimulationOutput {
    pub result: SimulationResult,
    pub simulation_via_rpc_err: Option<SimulateViaRpcError>,
}

#[derive(Debug)]
pub struct BothSimulationsFailed {
    pub rpc_error: SimulateViaRpcError,
    pub revm_error: SimulateViaRevmError,
}

impl std::fmt::Display for BothSimulationsFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "both RPC and REVM simulations failed")?;

        // Format RPC error chain (REVM chain will be handled by source())
        write!(f, "\n  RPC error: {}", self.rpc_error)?;
        let mut rpc_source = std::error::Error::source(&self.rpc_error);
        while let Some(source) = rpc_source {
            write!(f, "\n    caused by: {}", source)?;
            rpc_source = std::error::Error::source(source);
        }

        Ok(())
    }
}

impl std::error::Error for BothSimulationsFailed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.revm_error)
    }
}

#[derive(Debug, Error)]
pub enum SimulateError {
    #[error("failed to find balance slot")]
    FindSlot(#[from] FindSlotError),
    #[error("RPC error while getting block number")]
    Rpc(#[from] RpcError<TransportErrorKind>),
    #[error(transparent)]
    BothSimulationsFailed(#[from] BothSimulationsFailed),
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            db_caches: HashMap::new(),
        }
    }

    pub async fn simulate(
        &mut self,
        chain_id: u32,
        rpc_url: Url,
        params: SimulationParams,
    ) -> Result<SimulationOutput, SimulateError> {
        let cache = self.db_caches.entry(chain_id).or_default();

        let provider = ProviderBuilder::new().connect_http(rpc_url.clone());

        let block_number = provider.get_block_number().await?;
        let block_number = BlockId::number(block_number);

        let alloy_db = AlloyDB::new(provider, block_number);
        let alloy_db = WrapDatabaseAsync::new(alloy_db).expect("No Tokio runtime");

        let mut alloy_cache_db = CacheDB::new(alloy_db);

        //TODO: bug: if there is second simulation with the same rpc url there will be used
        //not cached state
        alloy_cache_db.cache = std::mem::take(cache);

        let balance_slot = find_balance_slot(params.token_in, params.user, &mut alloy_cache_db)?;

        let result: Result<SimulationOutput, SimulateError> =
            match simulate_via_rpc(&params, rpc_url, &balance_slot).await {
                Ok(rpc_result) => Ok(SimulationOutput {
                    result: rpc_result,
                    simulation_via_rpc_err: None,
                }),
                Err(rpc_error) => {
                    match simulate_via_revm(&params, &mut alloy_cache_db, balance_slot) {
                        Ok(revm_result) => Ok(SimulationOutput {
                            result: revm_result,
                            simulation_via_rpc_err: Some(rpc_error),
                        }),
                        Err(revm_error) => Err(BothSimulationsFailed {
                            rpc_error,
                            revm_error,
                        }
                        .into()),
                    }
                }
            };

        *cache = alloy_cache_db.cache;

        cache.accounts.iter_mut().for_each(|(_, db_account)| {
            db_account.storage.clear();
        });

        result
    }
}

#[derive(Debug, Error)]
#[error("simulation via revm failed")]
enum ApproveError {
    LoadAccount(#[from] DBTransportError),
    Transact(#[from] EVMError<DBTransportError>),
    #[error("execution failed: {0:?}")]
    Execution(ExecutionResult),
}

fn approve(
    token: Address,
    spender: Address,
    user: Address,
    alloy_cache_db: &mut AlloyCacheDb,
) -> Result<(), ApproveError> {
    let encoded = approveCall {
        spender,
        value: U256::MAX,
    }
    .abi_encode();

    let nonce = alloy_cache_db.load_account(user)?.info.nonce;

    let approve_tx_env = TxEnv::builder()
        .kind(TxKind::Call(token))
        .data(encoded.into())
        .caller(user)
        .nonce(nonce)
        .build_fill();

    let mut evm = Context::mainnet().with_db(alloy_cache_db).build_mainnet();

    let approve_res = evm.transact_commit(approve_tx_env)?;

    match approve_res {
        ExecutionResult::Success {
            reason: SuccessReason::Return,
            ..
        } => Ok(()),
        failed => Err(ApproveError::Execution(failed)),
    }
}

#[derive(Debug, Error)]
pub enum SimulateViaRpcError {
    #[error("eth_callMany call failed")]
    EthCallMany(#[from] crate::eth_call_many::EthCallManyError),
    #[error("approve transaction failed: {0}")]
    ApproveFailed(String),
    #[error("no valid response from simulation")]
    NoResponse,
}

#[derive(Debug, Error)]
#[error("simulation via revm failed")]
pub enum SimulateViaRevmError {
    LoadAccount(#[from] DBTransportError),
    Approve(#[from] ApproveError),
    #[error("execution failed: {0:?}")]
    Execution(ExecutionResult),
    Transact(#[from] EVMError<DBTransportError>),
}

fn simulate_via_revm(
    params: &SimulationParams,
    alloy_cache_db: &mut AlloyCacheDb,
    balance_slot: SlotWithAddress,
) -> Result<SimulationResult, SimulateViaRevmError> {
    let account = alloy_cache_db.load_account(balance_slot.address)?;
    account.storage.insert(balance_slot.slot, params.amount_in);

    approve(params.token_in, params.to, params.user, alloy_cache_db)?;

    let nonce = alloy_cache_db.load_account(params.user)?.info.nonce;

    let mut evm = Context::mainnet().with_db(alloy_cache_db).build_mainnet();

    let tx_env = TxEnv::builder()
        .kind(TxKind::Call(params.to))
        .data(params.calldata.clone())
        .caller(params.user)
        .nonce(nonce)
        .build_fill();

    let res = evm.transact_one(tx_env)?;

    match res {
        ExecutionResult::Success {
            reason: SuccessReason::Return,
            output,
            ..
        } => Ok(Ok(output.into_data())),
        failed => Ok(Err(format!("{:?}", failed))),
    }
}

async fn simulate_via_rpc(
    params: &SimulationParams,
    rpc_url: Url,
    balance_slot: &SlotWithAddress,
) -> Result<SimulationResult, SimulateViaRpcError> {
    let client = alloy_rpc_client::RpcClient::new_http(rpc_url);
    let eth_call_many = EthCallMany::new(&client);

    let mut storage = HashMap::new();
    storage.insert(balance_slot.slot.into(), params.amount_in.into());

    let state_override = StateOverride {
        state_diff: Some(storage),
        ..Default::default()
    };

    let mut state_overrides = HashMap::new();
    state_overrides.insert(params.token_in, state_override);

    let approve_data = approveCall {
        spender: params.to,
        value: U256::MAX,
    }
    .abi_encode()
    .into();

    let approve_tx = Transaction {
        from: Some(params.user),
        to: Some(params.token_in),
        data: Some(approve_data),
        ..Default::default()
    };

    let call_tx = Transaction {
        from: Some(params.user),
        to: Some(params.to),
        data: Some(params.calldata.clone()),
        ..Default::default()
    };

    let bundle = Bundle {
        transactions: vec![approve_tx, call_tx],
        block_override: None,
    };

    let simulation_context = SimulationContext {
        block_number: BlockId::latest(),
        transaction_index: None,
    };

    let result = eth_call_many
        .call_many(
            vec![bundle],
            simulation_context,
            Some(state_overrides),
            Some(5000),
        )
        .await?;

    let tx_responses = &result[0];

    for (idx, tx_response) in tx_responses.iter().enumerate() {
        match tx_response {
            TransactionResponse::Success { value, .. } => {
                if idx == 1 {
                    // Return the output from the second transaction (the actual call)
                    return Ok(Ok(value.clone()));
                }
            }
            TransactionResponse::Error { error } => {
                if idx == 1 {
                    // The main transaction reverted
                    return Ok(Err(error.clone()));
                } else {
                    // Approve transaction failed - this is an error
                    return Err(SimulateViaRpcError::ApproveFailed(error.clone()));
                }
            }
        }
    }

    Err(SimulateViaRpcError::NoResponse)
}
