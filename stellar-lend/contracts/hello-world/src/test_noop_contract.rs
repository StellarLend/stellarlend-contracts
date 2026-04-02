use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct NoopContract;

#[contractimpl]
impl NoopContract {
    pub fn noop(_env: Env) {}
}
