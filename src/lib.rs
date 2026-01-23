mod balance_slot;
mod eth_call_many;
mod simulator;

use alloy::transports::http::reqwest::Url;
use napi::bindgen_prelude::Either6;
use napi_derive::napi;
use revm::primitives::{Address, Bytes, U256};

use crate::simulator::{
    RevmTransactionResult, RpcTransactionResult, SimulateViaRevmError, SimulateViaRpcError,
    SimulationParams, SimulationResult, Simulator as SimulatorImpl,
};

#[napi(object)]
pub struct RpcSuccess {
    #[napi(ts_type = "'rpc_success'")]
    pub status: String,
    pub output: String,
}

#[napi(object)]
pub struct RpcRevert {
    #[napi(ts_type = "'rpc_revert'")]
    pub status: String,
    pub revert_reason: String,
}

#[napi(object)]
pub struct RpcFailedRevmSuccess {
    #[napi(ts_type = "'rpc_failed_revm_success'")]
    pub status: String,
    pub output: String,
    pub rpc_error: String,
}

#[napi(object)]
pub struct RpcFailedRevmRevert {
    #[napi(ts_type = "'rpc_failed_revm_revert'")]
    pub status: String,
    pub rpc_error: String,
    pub execution_result: String,
}

#[napi(object)]
pub struct BothFailed {
    #[napi(ts_type = "'both_failed'")]
    pub status: String,
    pub rpc_error: String,
    pub revm_error: String,
}

#[napi(object)]
pub struct Error {
    #[napi(ts_type = "'error'")]
    pub status: String,
    pub error: String,
}

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

    #[napi(
        ts_return_type = "Promise<RpcSuccess | RpcRevert | RpcFailedRevmSuccess | RpcFailedRevmRevert | BothFailed | Error>"
    )]
    pub async unsafe fn simulate(
        &mut self,
        user_address: String,
        token_in_address: String,
        to_address: String,
        calldata: String,
        chain_id: u32,
        rpc_url: String,
        amount_in: String,
    ) -> napi::Result<
        Either6<RpcSuccess, RpcRevert, RpcFailedRevmSuccess, RpcFailedRevmRevert, BothFailed, Error>,
    > {
        let rpc_url: Url = match rpc_url.parse() {
            Ok(url) => url,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid RPC URL: {}", e),
            })),
        };

        let to_address: Address = match to_address.parse() {
            Ok(addr) => addr,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid to address: {}", e),
            })),
        };

        let token_in_address: Address = match token_in_address.parse() {
            Ok(addr) => addr,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid token address: {}", e),
            })),
        };

        let user_address: Address = match user_address.parse() {
            Ok(addr) => addr,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid user address: {}", e),
            })),
        };

        let calldata: Bytes = match calldata.parse() {
            Ok(data) => data,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid calldata: {}", e),
            })),
        };

        let amount_in: U256 = match amount_in.parse() {
            Ok(amount) => amount,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("Invalid amount in: {}", e),
            })),
        };

        let params = SimulationParams {
            user: user_address,
            token_in: token_in_address,
            to: to_address,
            calldata,
            amount_in,
        };

        let result = match self.inner.simulate(chain_id, rpc_url, params).await {
            Ok(result) => result,
            Err(e) => return Ok(Either6::F(Error {
                status: "error".to_string(),
                error: format!("{:?}", anyhow::Error::from(e)),
            })),
        };

        let ts_result = match result {
            SimulationResult::Rpc(RpcTransactionResult::Success(output)) => {
                Either6::A(RpcSuccess {
                    status: "rpc_success".to_string(),
                    output: output.to_string(),
                })
            }
            SimulationResult::Rpc(RpcTransactionResult::Revert(reason)) => Either6::B(RpcRevert {
                status: "rpc_revert".to_string(),
                revert_reason: reason,
            }),
            SimulationResult::RpcFailedButRevm {
                rpc_error,
                revm_result: RevmTransactionResult::Success(output),
            } => Either6::C(RpcFailedRevmSuccess {
                status: "rpc_failed_revm_success".to_string(),
                output: output.to_string(),
                rpc_error: format!("{:?}", anyhow::Error::from(rpc_error)),
            }),
            SimulationResult::RpcFailedButRevm {
                rpc_error,
                revm_result: RevmTransactionResult::Failed(execution_result),
            } => Either6::D(RpcFailedRevmRevert {
                status: "rpc_failed_revm_revert".to_string(),
                rpc_error: format!("{:?}", anyhow::Error::from(rpc_error)),
                execution_result: format!("{:#?}", execution_result),
            }),
            SimulationResult::BothFailed {
                rpc_error,
                revm_error,
            } => Either6::E(BothFailed {
                status: "both_failed".to_string(),
                rpc_error: format!("{:?}", anyhow::Error::from(rpc_error)),
                revm_error: format!("{:?}", anyhow::Error::from(revm_error)),
            }),
        };

        Ok(ts_result)
    }
}
