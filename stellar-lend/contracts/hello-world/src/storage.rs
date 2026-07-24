use soroban_sdk::{contracttype, Address, Vec};

/// Guardian configuration for social-recovery authorisation.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardianConfig {
    /// Set of addresses authorised as recovery guardians.
    pub guardians: Vec<Address>,
    /// Number of guardian approvals required to execute a recovery.
    pub threshold: u32,
}
