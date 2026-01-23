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
use anyhow::{Result, anyhow};
use revm::{
    Context, ExecuteEvm, InspectEvm, Inspector, MainBuilder, MainContext,
    bytecode::{bitvec::store, opcode},
    context::{BlockEnv, CfgEnv, Evm, TxEnv, result::EVMError, tx::TxEnvBuildError},
    context_interface::result::ExecutionResult,
    database::{AlloyDB, CacheDB, DBTransportError, WrapDatabaseAsync},
    handler::{EthFrame, EthPrecompiles, instructions::EthInstructions},
    inspector::JournalExt,
    interpreter::{
        CallInputs, CallOutcome, Interpreter, InterpreterTypes, interpreter::EthInterpreter,
        interpreter_types::Jumps,
    },
    primitives::{HashSet, TxKind, address},
};

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

type ContextAlloyDb = Context<BlockEnv, TxEnv, CfgEnv, AlloyCacheDb>;
#[derive(Default)]
struct MyInspector {
    gas_used: u64,
    call_count: usize,
}

impl<CTX, INTR: InterpreterTypes> Inspector<CTX, INTR> for MyInspector {
    fn step(&mut self, interp: &mut Interpreter<INTR>, _context: &mut CTX) {
        self.gas_used += interp.gas.spent();
    }

    fn call(&mut self, _context: &mut CTX, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.call_count += 1;
        None // Don't override the call
    }
}

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
    fn step(&mut self, interp: &mut Interpreter<EthInterpreter>, context: &mut CTX) {
        let opcode = interp.bytecode.opcode();

        if opcode != SLOAD_OPCODE {
            return ();
        };

        interp.stack.peek(0).ok().inspect(|storage_slot| {
            // interp.
            self.slots.insert(SlotWithAddress {
                address: self.current_address,
                slot: *storage_slot,
            });
        });
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.current_address = inputs.target_address;
        None
    }
}

type EvmAlloyDb = Evm<
    ContextAlloyDb,
    SloadInspector,
    EthInstructions<EthInterpreter, ContextAlloyDb>,
    EthPrecompiles,
    EthFrame,
>;

#[tokio::main]
async fn main() -> Result<()> {
    let rpc_url: Url = "https://eth.drpc.org".parse()?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let alloy_db = WrapDatabaseAsync::new(AlloyDB::new(provider, BlockId::latest()))
        .expect("No Tokio runtime");

    let mut cache_db = CacheDB::new(alloy_db);

    let zro_address = address!("0x6985884C4392D348587B19cb9eAAf157F13271cd");
    let zro_holder_address = address!("0x1d4A12A09C293b816BFF3625abdA3Ae07dee19F5");
    let usdc_address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    let usdc_holder = address!("0xB166b43B24c2e42A12b2F788Ae0EFA536A914530");

    let zro_slot = find_balance_slot(zro_address, zro_holder_address, &mut cache_db);
    let usdc_slot = find_balance_slot(usdc_address, usdc_holder, &mut cache_db);

    println!("zro {zro_slot:?}");
    println!("usdc {usdc_slot:?}");

    Ok(())
}

fn balance_of(
    user_address: Address,
    token_address: Address,
    cache_db: &mut AlloyCacheDb,
) -> Result<U256> {
    let mut evm = Context::mainnet().with_db(cache_db).build_mainnet();

    let encoded = balanceOfCall {
        account: user_address,
    }
    .abi_encode();

    let result = evm.transact_one(
        TxEnv::builder()
            .kind(TxKind::Call(token_address))
            .data(encoded.into())
            .build()
            .expect("error building"),
    )?;

    let output = match result {
        ExecutionResult::Success { output, .. } => output,
        result => return Err(anyhow!("balanceOf call failed, reason {result:?}")),
    };

    let balance = U256::abi_decode(output.data())?;

    Ok(balance)
}

#[derive(Debug)]
enum BalanceSlotError {
    TxEnvBuildError,
    InspectError(EVMError<DBTransportError>),
    DBTransportError,
    SlotNotFound,
}

fn inspect_balance_of(
    token_address: Address,
    user_address: Address,
    cache_db: &mut AlloyCacheDb,
) -> Result<SloadInspector, BalanceSlotError> {
    let inspector = SloadInspector::default();

    let mut evm = Context::mainnet()
        .with_db(cache_db)
        .build_mainnet_with_inspector(inspector);

    let encoded = balanceOfCall {
        account: user_address,
    }
    .abi_encode();

    let tx = TxEnv::builder()
        .kind(TxKind::Call(token_address))
        .data(encoded.into())
        .build()
        .map_err(|_| BalanceSlotError::TxEnvBuildError)?;

    evm.inspect_one_tx(tx)
        .map_err(|err| BalanceSlotError::InspectError(err))?;

    Ok(evm.inspector)
}

fn find_balance_slot(
    token_address: Address,
    user_address: Address,
    cache_db: &mut AlloyCacheDb,
) -> Result<SlotWithAddress, BalanceSlotError> {
    let random_value = U256::from(1234567890);

    let inspector = inspect_balance_of(token_address, user_address, cache_db).unwrap();

    let balance_slot = inspector.slots.iter().find(|slot| {
        let acc = cache_db.load_account(slot.address);

        let Ok(acc) = acc else {
            return false;
        };

        let original_value = acc.storage.get(&slot.slot);

        let Some(original_value) = original_value else {
            return false;
        };
        let original_value = original_value.clone();

        acc.storage.insert(slot.slot, random_value);

        let new_balance = balance_of(user_address, token_address, cache_db);

        let Ok(new_balance) = new_balance else {
            return false;
        };

        let acc = cache_db.load_account(slot.address);

        let Ok(acc) = acc else {
            return false;
        };

        acc.storage.insert(slot.slot, original_value);

        if new_balance == random_value {
            return true;
        }

        return false;
    });

    match balance_slot {
        Some(balance_slot) => Ok(balance_slot.clone()),
        None => Err(BalanceSlotError::SlotNotFound),
    }
}
