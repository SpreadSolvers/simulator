mod balance_slot;

use std::time::Instant;

use alloy::{
    eips::BlockId,
    providers::{Provider, ProviderBuilder},
    transports::http::reqwest::Url,
};
use alloy_rpc_client::ClientBuilder;
use revm::{
    database::{AlloyDB, CacheDB, WrapDatabaseAsync},
    primitives::address,
};

use crate::balance_slot::find_balance_slot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let rpc_url: Url = "https://rpc.flashbots.net".parse()?;
    // let rpc_url: Url = "https://rrpc.flashbots.net".parse()?;

    let usdc_address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    let usdc_holder = address!("0xB166b43B24c2e42A12b2F788Ae0EFA536A914530");

    // Create a client with logging layer
    let client = ClientBuilder::default().http(rpc_url.clone());

    // Create provider with the logging client
    let provider = ProviderBuilder::new().connect_client(client);

    let block_number = provider.get_block_number().await?;
    let block_id = BlockId::number(block_number);

    // Create AlloyDB with the provider that has logging
    let alloy_db =
        WrapDatabaseAsync::new(AlloyDB::new(provider, block_id)).expect("No Tokio runtime");

    let mut alloy_cache_db = CacheDB::new(alloy_db);

    println!("Finding balance slot for USDC...");

    let start = Instant::now();

    let usdc_slot = find_balance_slot(usdc_address, usdc_holder, &mut alloy_cache_db)?;

    println!("USDC slot: {usdc_slot:?}");
    println!("time taken: {:?}", start.elapsed());

    Ok(())
}
