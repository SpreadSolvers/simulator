use alloy::{
    network::Ethereum,
    primitives::{Address, U256},
    providers::{
        Identity, RootProvider,
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
    },
    sol,
    sol_types::{SolCall, SolValue},
};
use revm::{
    Context, ExecuteEvm, InspectEvm, Inspector, MainBuilder, MainContext,
    context::{
        TxEnv,
        result::{EVMError, SuccessReason},
        tx::TxEnvBuildError,
    },
    context_interface::result::ExecutionResult,
    database::{AlloyDB, CacheDB, DBTransportError, EmptyDB, WrapDatabaseAsync},
    interpreter::{
        CallInputs, CallOutcome, Interpreter, interpreter::EthInterpreter, interpreter_types::Jumps,
    },
    primitives::{HashSet, TxKind},
};
use std::convert::Infallible;
use thiserror::Error;

use crate::balance_slot::IERC20::balanceOfCall;

sol!(
    #[sol(rpc)]
    "artifacts/erc20.sol"
);

pub type AlloyCacheDb = CacheDB<
    WrapDatabaseAsync<
        AlloyDB<
            Ethereum,
            FillProvider<
                JoinFill<
                    Identity,
                    JoinFill<
                        GasFiller,
                        JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>,
                    >,
                >,
                RootProvider,
            >,
        >,
    >,
>;

const SLOAD_OPCODE: u8 = 0x54;

#[derive(Eq, Hash, PartialEq, Clone, Debug)]
pub struct SlotWithAddress {
    pub address: Address,
    pub slot: U256,
}

#[derive(Default)]
struct SloadInspector {
    slots: HashSet<SlotWithAddress>,
    current_address: Address,
}

impl<CTX> Inspector<CTX> for SloadInspector {
    fn step(&mut self, interp: &mut Interpreter<EthInterpreter>, _: &mut CTX) {
        let opcode = interp.bytecode.opcode();

        if opcode != SLOAD_OPCODE {
            return ();
        };

        interp.stack.peek(0).ok().inspect(|storage_slot| {
            self.slots.insert(SlotWithAddress {
                address: self.current_address,
                slot: *storage_slot,
            });
        });
    }

    fn call(&mut self, _: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.current_address = inputs.target_address;
        None
    }
}

#[derive(Debug, Error)]
#[error("getting balance failed")]
enum BalanceOfError {
    TxBuild(TxEnvBuildError),
    TransactOne(#[from] EVMError<Infallible>),
    Execution(ExecutionResult),
    Decoding(#[from] alloy::sol_types::Error),
}

impl From<TxEnvBuildError> for BalanceOfError {
    fn from(value: TxEnvBuildError) -> Self {
        BalanceOfError::TxBuild(value)
    }
}

impl From<ExecutionResult> for BalanceOfError {
    fn from(value: ExecutionResult) -> Self {
        BalanceOfError::Execution(value)
    }
}

fn balance_of(
    user_address: Address,
    token_address: Address,
    cache_db: &mut CacheDB<EmptyDB>,
) -> Result<U256, BalanceOfError> {
    let mut evm = Context::mainnet()
        .with_db(cache_db)
        .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
        .build_mainnet();

    let tx_env = build_balance_of_tx_env(token_address, user_address)?;

    let result = evm.transact_one(tx_env)?;

    //TODO: check reason = return
    let output = match result {
        ExecutionResult::Success { output, .. } => output,
        result => return Err(BalanceOfError::Execution(result)),
    };

    let balance = U256::abi_decode(output.data())?;

    Ok(balance)
}

#[derive(Debug, Error)]
#[error("finding balance slot failed")]
pub enum FindSlotError {
    FindSlotByMutation(#[from] FindSlotByMutationError),
    InspectBalanceOf(#[from] InspectBalanceOfError),
}

#[derive(Debug, Error)]
#[error("inspecting balanceOf call failed")]
pub enum InspectBalanceOfError {
    TxBuild(TxEnvBuildError),
    InspectError(#[from] EVMError<DBTransportError>),
    #[error("execution failed: {0:?}")]
    Execution(ExecutionResult),
}

impl From<TxEnvBuildError> for InspectBalanceOfError {
    fn from(value: TxEnvBuildError) -> Self {
        InspectBalanceOfError::TxBuild(value)
    }
}

fn inspect_balance_of(
    token_address: Address,
    user_address: Address,
    cache_db: &mut AlloyCacheDb,
) -> Result<SloadInspector, InspectBalanceOfError> {
    let inspector = SloadInspector::default();

    let mut evm = Context::mainnet()
        .with_db(cache_db)
        .modify_cfg_chained(|cfg| cfg.disable_nonce_check = true)
        .build_mainnet_with_inspector(inspector);

    let tx = build_balance_of_tx_env(token_address, user_address)?;

    let res = evm.inspect_one_tx(tx)?;

    match res {
        ExecutionResult::Success {
            reason: SuccessReason::Return,
            ..
        } => Ok(evm.inspector),
        failed => Err(InspectBalanceOfError::Execution(failed)),
    }
}

fn build_balance_of_tx_env(
    token_address: Address,
    user_address: Address,
) -> Result<TxEnv, TxEnvBuildError> {
    let encoded = balanceOfCall {
        account: user_address,
    }
    .abi_encode();

    let tx_env = TxEnv::builder()
        .kind(TxKind::Call(token_address))
        .data(encoded.into())
        .build()?;

    Ok(tx_env)
}

pub fn find_balance_slot(
    token_address: Address,
    user_address: Address,
    alloy_cache_db: &mut AlloyCacheDb,
) -> Result<SlotWithAddress, FindSlotError> {
    let inspector = inspect_balance_of(token_address, user_address, alloy_cache_db)?;

    //TODO: remove clone
    let cached_accounts = alloy_cache_db.cache.accounts.clone();

    let mut isolated_db = CacheDB::new(EmptyDB::default());
    isolated_db.cache.accounts = cached_accounts;

    let slot_with_address =
        find_slot_by_mutation(user_address, token_address, &inspector, &mut isolated_db)?;

    Ok(slot_with_address)
}

const TARGET_VALUE: U256 = U256::from_limbs([1234567890, 0, 0, 0]);

#[derive(Debug, Error)]
#[error("finding slot by mutation failed")]
pub struct FindSlotByMutationError;

fn find_slot_by_mutation(
    user_address: Address,
    token_address: Address,
    inspector: &SloadInspector,
    cache_db: &mut CacheDB<EmptyDB>,
) -> Result<SlotWithAddress, FindSlotByMutationError> {
    for slot_with_address in inspector.slots.iter() {
        let new_balance = test_slot(user_address, token_address, slot_with_address, cache_db);

        if let Ok(new_balance) = new_balance {
            if new_balance == TARGET_VALUE {
                return Ok(slot_with_address.clone());
            }
        }
    }

    Err(FindSlotByMutationError)
}

#[derive(Debug, Error)]
#[error("testing slot failed")]
enum TestSlotError {
    BalanceOf(#[from] BalanceOfError),
    Infallible(#[from] Infallible),
}

fn test_slot(
    user_address: Address,
    token_address: Address,
    slot_with_address: &SlotWithAddress,
    cache_db: &mut CacheDB<EmptyDB>,
) -> Result<U256, TestSlotError> {
    let acc = cache_db.load_account(slot_with_address.address)?;

    let original_value = acc.storage.get(&slot_with_address.slot).copied();

    acc.storage.insert(slot_with_address.slot, TARGET_VALUE);

    let new_balance = balance_of(user_address, token_address, cache_db);

    let acc = cache_db
        .load_account(slot_with_address.address)
        .expect("never fail");

    match original_value {
        Some(original_value) => {
            acc.storage.insert(slot_with_address.slot, original_value);
        }
        None => {
            acc.storage.remove(&slot_with_address.slot);
        }
    }

    Ok(new_balance?)
}

#[cfg(test)]
mod tests {
    use alloy::{
        eips::BlockId,
        providers::{Provider, ProviderBuilder},
    };
    use revm::primitives::address;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_find_balance_slot() -> Result<(), Box<dyn std::error::Error>> {
        dotenvy::dotenv().ok();
        let rpc_url = std::env::var("BASE_RPC")
            .expect("BASE_RPC not set in .env")
            .parse()?;

        let provider = ProviderBuilder::new().connect_http(rpc_url);

        let block_number = provider.get_block_number().await?;
        let block_number = BlockId::number(block_number);

        let alloy_db = AlloyDB::new(provider, block_number);
        let alloy_db = WrapDatabaseAsync::new(alloy_db).ok_or("No Tokio runtime available")?;

        let mut alloy_cache_db = CacheDB::new(alloy_db);

        let user = address!("0x6698192C6e70186ebE73E2785aC85a8f5B85b052");

        let token = address!("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");

        let slot = find_balance_slot(token, user, &mut alloy_cache_db)?;

        println!("Found balance slot: {:?}", slot);

        Ok(())
    }
}
