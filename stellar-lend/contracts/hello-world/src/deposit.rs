use soroban_sdk::{contracttype, Address, Env};

/// Storage keys for protocol reserve balances and deposit operations.
///
/// The [`ProtocolReserve`] variant tracks per-asset reserve accumulations
/// that can be claimed by the protocol admin via [`claim_reserves`].
///
/// [`claim_reserves`]: crate::HelloContract::claim_reserves
#[contracttype]
pub enum DepositDataKey {
    /// Accumulated protocol reserve for an asset.
    /// Value type: `i128`.
    ProtocolReserve(Option<Address>),
}

/// Deposit collateral for a user.
pub fn deposit_collateral(env: &Env, _caller: Address, _asset: Option<Address>, _amount: i128) {}

