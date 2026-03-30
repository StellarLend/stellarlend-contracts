use soroban_sdk::{token, Address, Env};

pub fn create_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    let token_client = token::StellarAssetClient::new(env, &token_address);
    (token_address, token_client)
}

pub fn create_token_and_mint(env: &Env, admin: &Address, user: &Address, amount: i128) -> (Address, token::StellarAssetClient<'static>) {
    let (asset, client) = create_token(env, admin);
    client.mint(user, &amount);
    (asset, client)
}
