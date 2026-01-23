#![deny(clippy::all)]
mod balance_slot;

use std::time::Instant;

use alloy::{
    eips::BlockId, providers::ProviderBuilder, sol_types::SolCall, transports::http::reqwest::Url,
};
use napi_derive::napi;
use revm::{
    Context, ExecuteCommitEvm, ExecuteEvm, MainBuilder, MainContext,
    context::{
        TxEnv,
        result::{ExecutionResult, SuccessReason},
    },
    database::{AlloyDB, CacheDB, WrapDatabaseAsync},
    handler::post_execution::output,
    primitives::{Address, TxKind, U256},
};

use crate::balance_slot::{IERC20::approveCall, find_balance_slot};

#[tokio::main]
#[napi]
pub async fn simulate(
    user_address: String,
    token_in_address: String,
    to_address: String,
    calldata: String,
    rpc_url: String,
    amount_in: String,
) -> () {
    let rpc_url: Url = rpc_url.parse().unwrap();
    // let rpc_url: Url = "http://127.0.0.1:8545".parse().unwrap();

    let to_address: Address = to_address.parse().unwrap();
    let token_in_address: Address = token_in_address.parse().unwrap();
    let user_address: Address = user_address.parse().unwrap();
    let amount_in: U256 = amount_in.parse().unwrap();
    let calldata = calldata.parse().unwrap();

    let provider = ProviderBuilder::new().connect_http(rpc_url.clone());

    let alloy_db = WrapDatabaseAsync::new(AlloyDB::new(provider, BlockId::latest()))
        .expect("No Tokio runtime");

    let mut alloy_cache_db = CacheDB::new(alloy_db);

    let find_slot_start = Instant::now();

    let balance_slot = find_balance_slot(
        token_in_address,
        user_address,
        rpc_url.clone(),
        &mut alloy_cache_db,
    )
    .unwrap();

    println!("find slot time: {:?}", find_slot_start.elapsed());

    let acc = alloy_cache_db.load_account(balance_slot.address).unwrap();

    acc.storage.insert(balance_slot.slot, amount_in);

    let encoded = approveCall {
        spender: to_address,
        value: amount_in,
    }
    .abi_encode();

    let nonce = alloy_cache_db
        .load_account(user_address)
        .unwrap()
        .info
        .nonce;

    let approve_tx_env = TxEnv::builder()
        .kind(TxKind::Call(token_in_address))
        .data(encoded.into())
        .caller(user_address)
        .nonce(nonce)
        .build()
        .unwrap();

    let nonce = alloy_cache_db
        .load_account(user_address)
        .unwrap()
        .info
        .nonce;

    let tx_env = TxEnv::builder()
        .kind(TxKind::Call(to_address))
        .data(calldata)
        .caller(user_address)
        .nonce(nonce + 1)
        .build()
        .unwrap();

    let mut evm = Context::mainnet().with_db(alloy_cache_db).build_mainnet();

    let approve_tx_start = Instant::now();

    let approve_res = evm.transact_commit(approve_tx_env).unwrap();

    let output = match approve_res {
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

    println!("approve tx time: {:?}", approve_tx_start.elapsed());
    dbg!(output);

    let tx_start = Instant::now();

    let result = evm.transact_one(tx_env).unwrap();

    let output = match result {
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

    println!("lifi tx time: {:?}", tx_start.elapsed());

    dbg!(output);
}
