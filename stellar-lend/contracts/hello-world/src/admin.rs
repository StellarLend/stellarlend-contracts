use soroban_sdk::{Address, Env};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AdminError {
    Unauthorized,
}

pub fn has_admin(_env: &Env) -> bool {
    false
}

pub fn set_admin(_env: &Env, _admin: Address, _caller: Option<Address>) -> Result<(), AdminError> {
    Ok(())
}
