use alloy::{
    eips::BlockId,
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall,
    transports::http::reqwest::Url,
};
use revm::{
    Context, ExecuteCommitEvm, ExecuteEvm, MainBuilder, MainContext,
    context::{
        TxEnv,
        result::{ExecutionResult, SuccessReason},
    },
    database::{AlloyDB, Cache, CacheDB, WrapDatabaseAsync},
    primitives::{Address, Bytes, TxKind, U256},
};
use std::collections::HashMap;

use crate::balance_slot::{AlloyCacheDb, IERC20::approveCall, find_balance_slot};

pub struct Simulator {
    db_caches: HashMap<u32, Cache>,
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            db_caches: HashMap::new(),
        }
    }

    pub async fn simulate(
        &mut self,
        user: Address,
        token_in: Address,
        to: Address,
        calldata: Bytes,
        chain_id: u32,
        rpc_url: Url,
        amount_in: String,
    ) -> () {
        let cache = self.db_caches.entry(chain_id).or_default();

        let provider = ProviderBuilder::new().connect_http(rpc_url);

        let block_number = provider.get_block_number().await.unwrap();
        let block_number = BlockId::number(block_number);

        let alloy_db = AlloyDB::new(provider, block_number);
        let alloy_db = WrapDatabaseAsync::new(alloy_db).expect("No Tokio runtime");

        let mut alloy_cache_db = CacheDB::new(alloy_db);

        // Use the cached contract code/state from previous simulations
        // Take ownership temporarily, will be replaced after simulation
        alloy_cache_db.cache = std::mem::take(cache);

        let balance_slot = find_balance_slot(token_in, user, &mut alloy_cache_db).unwrap();

        // Set the user's token balance to amount_in
        let amount: U256 = amount_in.parse().unwrap();
        let account = alloy_cache_db.load_account(balance_slot.address).unwrap();
        account.storage.insert(balance_slot.slot, amount);

        approve(token_in, to, user, &mut alloy_cache_db).unwrap();
        simulate(to, calldata, user, &mut alloy_cache_db).unwrap();

        // Store the updated cache (including any newly fetched contract code)
        *cache = alloy_cache_db.cache;

        cache.accounts.iter_mut().for_each(|(_, db_account)| {
            db_account.storage.clear();
        });
    }
}

fn approve(
    token: Address,
    spender: Address,
    user: Address,
    alloy_cache_db: &mut AlloyCacheDb,
) -> Result<(), ()> {
    let encoded = approveCall {
        spender,
        value: U256::MAX,
    }
    .abi_encode();

    let nonce = alloy_cache_db.load_account(user).unwrap().info.nonce;

    let approve_tx_env = TxEnv::builder()
        .kind(TxKind::Call(token))
        .data(encoded.into())
        .caller(user)
        .nonce(nonce)
        .build()
        .unwrap();

    let mut evm = Context::mainnet().with_db(alloy_cache_db).build_mainnet();

    let approve_res = evm.transact_commit(approve_tx_env).unwrap();

    match approve_res {
        ExecutionResult::Success {
            reason: SuccessReason::Return,
            output,
            ..
        } => output,
        failed => {
            dbg!(failed);
            panic!()
        }
    };

    Ok(())
}

fn simulate(
    to: Address,
    calldata: Bytes,
    user: Address,
    alloy_cache_db: &mut AlloyCacheDb,
) -> Result<(), ()> {
    let nonce = alloy_cache_db.load_account(user).unwrap().info.nonce;

    let mut evm = Context::mainnet().with_db(alloy_cache_db).build_mainnet();

    let tx_env = TxEnv::builder()
        .kind(TxKind::Call(to))
        .data(calldata.clone())
        .caller(user)
        .nonce(nonce)
        .build_fill();

    let result = evm.transact_one(tx_env).unwrap();

    match result {
        ExecutionResult::Success {
            reason: SuccessReason::Return,
            output,
            ..
        } => output,
        failed => {
            panic!("Execution failed: {:?}", failed)
        }
    };

    Ok(())
}
