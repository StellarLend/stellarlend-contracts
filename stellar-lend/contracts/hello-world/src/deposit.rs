use soroban_sdk::{contracttype, Address, Env};

/// Storage keys used by the deposit module.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositDataKey {
    /// Protocol reserve balance for an asset (stroops).
    ProtocolReserve(Option<Address>),
}

/// Placeholder for protocol-level analytics derived from deposit data.
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct ProtocolAnalytics;

/// Deposit collateral into the protocol (stub — real logic lives in
/// cross-asset module).
pub fn deposit_collateral(_env: &Env, _asset: Option<Address>, _amount: i128) {}
