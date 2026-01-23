use std::convert::Infallible;

use alloy::{
    eips::BlockId,
    network::Ethereum,
    primitives::{Address, U256},
    providers::{
        Identity, ProviderBuilder, RootProvider,
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
    },
    sol,
    sol_types::{SolCall, SolValue},
    transports::http::reqwest::Url,
};
use revm::{
    Context, ExecuteEvm, InspectEvm, Inspector, MainBuilder, MainContext,
    context::{TxEnv, result::EVMError, tx::TxEnvBuildError},
    context_interface::result::ExecutionResult,
    database::{AlloyDB, CacheDB, DBTransportError, EmptyDB, WrapDatabaseAsync},
    interpreter::{
        CallInputs, CallOutcome, Interpreter, interpreter::EthInterpreter, interpreter_types::Jumps,
    },
    primitives::{HashSet, TxKind, address},
};
use thiserror::Error;

use crate::IERC20::balanceOfCall;

sol!(
    #[sol(rpc)]
    "artifacts/erc20.sol"
);

type AlloyCacheDb = CacheDB<
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
struct SlotWithAddress {
    address: Address,
    slot: U256,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let rpc_url: Url = "https://eth.drpc.org".parse()?;
    let rpc_url: Url = "https://ethereum-rpc.publicnode.com".parse()?;
    let rpc_url: Url = "https://rpc.flashbots.net".parse()?;

    let zro_address = address!("0x6985884C4392D348587B19cb9eAAf157F13271cd");
    let zro_holder_address = address!("0x1d4A12A09C293b816BFF3625abdA3Ae07dee19F5");
    let usdc_address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    let usdc_holder = address!("0xB166b43B24c2e42A12b2F788Ae0EFA536A914530");

    let zro_slot = find_balance_slot(zro_address, zro_holder_address, rpc_url.clone())?;
    println!("zro {zro_slot:?}");

    let usdc_slot = find_balance_slot(usdc_address, usdc_holder, rpc_url.clone())?;
    println!("usdc {usdc_slot:?}");

    Ok(())
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
    let mut evm = Context::mainnet().with_db(cache_db).build_mainnet();

    let tx_env = build_balance_of_tx_env(token_address, user_address)?;

    let result = evm.transact_one(tx_env)?;

    let output = match result {
        ExecutionResult::Success { output, .. } => output,
        result => return Err(BalanceOfError::Execution(result)),
    };

    let balance = U256::abi_decode(output.data())?;

    Ok(balance)
}

#[derive(Debug, Error)]
#[error("finding balance slot failed")]
enum FindSlotError {
    FindSlotByMutation(#[from] FindSlotByMutationError),
    InspectBalanceOf(#[from] InspectBalanceOfError),
}

#[derive(Debug, Error)]
#[error("inspecting balanceOf call failed")]
enum InspectBalanceOfError {
    TxBuild(TxEnvBuildError),
    InspectError(#[from] EVMError<DBTransportError>),
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
        .build_mainnet_with_inspector(inspector);

    let tx = build_balance_of_tx_env(token_address, user_address)?;

    evm.inspect_one_tx(tx)?;

    Ok(evm.inspector)
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

fn find_balance_slot(
    token_address: Address,
    user_address: Address,
    rpc_url: Url,
) -> Result<SlotWithAddress, FindSlotError> {
    let mut alloy_cache_db = create_alloy_db(rpc_url);

    let inspector = inspect_balance_of(token_address, user_address, &mut alloy_cache_db)?;

    let cached_accounts = alloy_cache_db.cache.accounts;

    let mut isolated_db = CacheDB::new(EmptyDB::default());
    isolated_db.cache.accounts = cached_accounts; // or use insert methods

    let slot_with_address =
        find_slot_by_mutation(user_address, token_address, &inspector, &mut isolated_db)?;

    Ok(slot_with_address)
}

fn create_alloy_db(rpc_url: Url) -> AlloyCacheDb {
    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let alloy_db = WrapDatabaseAsync::new(AlloyDB::new(provider, BlockId::latest()))
        .expect("No Tokio runtime");

    CacheDB::new(alloy_db)
}

const TARGET_VALUE: U256 = U256::from_limbs([1234567890, 0, 0, 0]);

#[derive(Debug, Error)]
#[error("finding slot by mutation failed")]
struct FindSlotByMutationError;

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

    let original_value = match acc.storage.get(&slot_with_address.slot) {
        Some(original_value) => Some(original_value.clone()),
        None => None,
    };

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
