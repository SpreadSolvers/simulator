mod balance_slot;
mod eth_call_many;
mod simulator;

use alloy::transports::http::reqwest::Url;
use napi::bindgen_prelude::Either3;
use napi_derive::napi;
use std::str::FromStr;

use crate::simulator::{SimulationParams as SimulationParamsInternal, Simulator as SimulatorImpl};

const STATUS_SUCCESS: &str = "simulation_success";
const STATUS_FAILED: &str = "simulation_failed";
const STATUS_ERROR: &str = "error";

fn parse_or_error<T: FromStr>(value: &str, field_name: &str) -> Result<T, Error>
where
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|e| Error {
        status: STATUS_ERROR.to_string(),
        error: format!("Invalid {}: {}", field_name, e),
    })
}

fn validate_and_convert(
    params: SimulationParams,
    rpc_url: String,
) -> Result<(SimulationParamsInternal, Url), Error> {
    let rpc_url = parse_or_error::<Url>(&rpc_url, "RPC URL")?;
    let simulation_params = params.try_into()?;
    Ok((simulation_params, rpc_url))
}

#[napi(object)]
pub struct SimulationParams {
    pub user_address: String,
    pub token_in_address: String,
    pub to_address: String,
    pub calldata: String,
    pub amount_in: String,
}

impl TryFrom<SimulationParams> for SimulationParamsInternal {
    type Error = Error;

    fn try_from(params: SimulationParams) -> Result<Self, Self::Error> {
        Ok(SimulationParamsInternal {
            user: parse_or_error(&params.user_address, "user address")?,
            token_in: parse_or_error(&params.token_in_address, "token address")?,
            to: parse_or_error(&params.to_address, "to address")?,
            calldata: parse_or_error(&params.calldata, "calldata")?,
            amount_in: parse_or_error(&params.amount_in, "amount in")?,
        })
    }
}

#[napi(object)]
pub struct SimulationSuccess {
    #[napi(ts_type = STATUS_SUCCESS)]
    pub status: String,
    pub output: String,
    pub rpc_err: Option<String>,
}

#[napi(object)]
pub struct SimulationFailed {
    #[napi(ts_type = STATUS_FAILED)]
    pub status: String,
    pub output: String,
    pub rpc_err: Option<String>,
}

#[napi(object)]
pub struct Error {
    #[napi(ts_type = STATUS_ERROR)]
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

    /// Simulates a transaction with token balance manipulation.
    ///
    /// **WARNING**: Not safe for concurrent calls - cache will be overwritten.
    /// Always await each call before starting the next one.
    #[napi(ts_return_type = "Promise<SimulationSuccess | SimulationFailed | Error>")]
    pub async unsafe fn simulate(
        &mut self,
        params: SimulationParams,
        chain_id: u32,
        rpc_url: String,
    ) -> napi::Result<Either3<SimulationSuccess, SimulationFailed, Error>> {
        let (simulation_params, rpc_url) = match validate_and_convert(params, rpc_url) {
            Ok(validated) => validated,
            Err(e) => return Ok(Either3::C(e)),
        };

        let output = match self
            .inner
            .simulate(chain_id, rpc_url, simulation_params)
            .await
        {
            Ok(output) => output,
            Err(e) => {
                return Ok(Either3::C(Error {
                    status: STATUS_ERROR.to_string(),
                    error: format!("{:#}", anyhow::Error::from(e)),
                }));
            }
        };

        let rpc_err = output
            .simulation_via_rpc_err
            .map(|e| format!("{:#}", anyhow::Error::from(e)));

        let ts_result = match output.result {
            Ok(bytes) => Either3::A(SimulationSuccess {
                status: STATUS_SUCCESS.to_string(),
                output: bytes.to_string(),
                rpc_err,
            }),
            Err(reason) => Either3::B(SimulationFailed {
                status: STATUS_FAILED.to_string(),
                output: reason,
                rpc_err,
            }),
        };

        Ok(ts_result)
    }
}
