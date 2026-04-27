#![no_std]
#![allow(deprecated)]
mod bridge;

pub use bridge::{BridgeConfig, BridgeContract, ContractError, MAX_FEE_BPS, MAX_ID_LEN};

#[cfg(test)]
mod bridge_coverage_booster;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod test;
