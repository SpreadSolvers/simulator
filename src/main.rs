mod balance_slot;

use alloy::{sol_types::SolCall, transports::http::reqwest::Url};
use revm::primitives::address;

use crate::balance_slot::find_balance_slot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let rpc_url: Url = "https://eth.drpc.org".parse()?;
    let rpc_url: Url = "https://ethereum-rpc.publicnode.com".parse()?;
    let rpc_url: Url = "https://rpc.flashbots.net".parse()?;
    // let rpc_url: Url = "https://rrpc.flashbots.net".parse()?;

    let zro_address = address!("0x6985884C4392D348587B19cb9eAAf157F13271cd");
    let zro_holder_address = address!("0x1d4A12A09C293b816BFF3625abdA3Ae07dee19F5");
    let usdc_address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    let usdc_holder = address!("0xB166b43B24c2e42A12b2F788Ae0EFA536A914530");
    let empty_address = address!("0x183722431Db2CFb8145C939ab3C6d759bE8CeDDe");

    // let slot = find_balance_slot(zro_address, empty_address, rpc_url.clone())?;

    // println!("slot {slot:?}");

    // let zro_slot = find_balance_slot(zro_address, zro_holder_address, rpc_url.clone())?;
    // println!("zro {zro_slot:?}");
    //
    // let usdc_slot = find_balance_slot(usdc_address, usdc_holder, rpc_url.clone())?;
    // println!("usdc {usdc_slot:?}");

    Ok(())
}
