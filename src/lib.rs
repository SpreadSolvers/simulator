mod balance_slot;
mod simulator;

use alloy::transports::http::reqwest::Url;
use napi_derive::napi;
use revm::primitives::{Address, Bytes};

use crate::simulator::Simulator as SimulatorImpl;

#[napi]
pub struct Simulator {
    inner: SimulatorImpl,
}

#[napi]
impl Simulator {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: SimulatorImpl::new(),
        }
    }

    #[napi]
    pub async unsafe fn simulate(
        &mut self,
        user_address: String,
        token_in_address: String,
        to_address: String,
        calldata: String,
        chain_id: u32,
        rpc_url: String,
        amount_in: String,
    ) -> napi::Result<()> {
        let rpc_url: Url = rpc_url
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid RPC URL: {}", e)))?;

        let to_address: Address = to_address
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid to address: {}", e)))?;
        let token_in_address: Address = token_in_address
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid token address: {}", e)))?;
        let user_address: Address = user_address
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid user address: {}", e)))?;
        let calldata: Bytes = calldata
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid calldata: {}", e)))?;

        self.inner
            .simulate(
                user_address,
                token_in_address,
                to_address,
                calldata,
                chain_id,
                rpc_url,
                amount_in,
            )
            .await;

        Ok(())
    }
}
