use soroban_sdk::{contracttype, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct FlashLoanConfig;

pub fn configure_flash_loan(_env: &Env) {}
pub fn execute_flash_loan(_env: &Env) {}
pub fn repay_flash_loan(_env: &Env) {}
pub fn set_flash_loan_fee(_env: &Env) {}
