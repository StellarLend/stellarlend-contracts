use soroban_sdk::{Address, Env, IntoVal, String, Symbol, Val, Vec};
use crate::types::*;

/// Emit swap event
pub fn emit_swap_event(
    env: &Env,
    user: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    amount_out: i128,
    protocol: &String,
    fee_paid: i128,
) {
    let topics = (Symbol::new(env, "amm_swap"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_in").into_val(env));
    data.push_back(token_in.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_out").into_val(env));
    data.push_back(token_out.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_in").into_val(env));
    data.push_back(amount_in.into_val(env));
    data.push_back(Symbol::new(env, "amount_out").into_val(env));
    data.push_back(amount_out.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));
    data.push_back(Symbol::new(env, "fee_paid").into_val(env));
    data.push_back(fee_paid.into_val(env));

    env.events().publish(topics, data);
}

/// Emit liquidity added event
pub fn emit_liquidity_added_event(
    env: &Env,
    user: &Address,
    token_a: &Address,
    token_b: &Address,
    amount_a: i128,
    amount_b: i128,
    liquidity_minted: i128,
    protocol: &String,
) {
    let topics = (Symbol::new(env, "amm_liquidity_added"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_a").into_val(env));
    data.push_back(token_a.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_b").into_val(env));
    data.push_back(token_b.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_a").into_val(env));
    data.push_back(amount_a.into_val(env));
    data.push_back(Symbol::new(env, "amount_b").into_val(env));
    data.push_back(amount_b.into_val(env));
    data.push_back(Symbol::new(env, "liquidity_minted").into_val(env));
    data.push_back(liquidity_minted.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));

    env.events().publish(topics, data);
}

/// Emit liquidity removed event
pub fn emit_liquidity_removed_event(
    env: &Env,
    user: &Address,
    token_a: &Address,
    token_b: &Address,
    amount_a: i128,
    amount_b: i128,
    liquidity_burned: i128,
    protocol: &String,
) {
    let topics = (Symbol::new(env, "amm_liquidity_removed"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_a").into_val(env));
    data.push_back(token_a.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_b").into_val(env));
    data.push_back(token_b.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_a").into_val(env));
    data.push_back(amount_a.into_val(env));
    data.push_back(Symbol::new(env, "amount_b").into_val(env));
    data.push_back(amount_b.into_val(env));
    data.push_back(Symbol::new(env, "liquidity_burned").into_val(env));
    data.push_back(liquidity_burned.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));

    env.events().publish(topics, data);
}

/// Emit callback validation event
pub fn emit_callback_validation_event(
    env: &Env,
    caller: &Address,
    operation: &Symbol,
    valid: bool,
) {
    let topics = (Symbol::new(env, "amm_callback_validation"), caller.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "caller").into_val(env));
    data.push_back(caller.clone().into_val(env));
    data.push_back(Symbol::new(env, "operation").into_val(env));
    data.push_back(operation.clone().into_val(env));
    data.push_back(Symbol::new(env, "valid").into_val(env));
    data.push_back(valid.into_val(env));

    env.events().publish(topics, data);
}

/// Emit hook execution event
pub fn emit_hook_execution_event(
    env: &Env,
    hook_name: &Symbol,
    target_contract: &Address,
    success: bool,
    operation: &Symbol,
) {
    let topics = (Symbol::new(env, "amm_hook_execution"), target_contract.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "hook_name").into_val(env));
    data.push_back(hook_name.clone().into_val(env));
    data.push_back(Symbol::new(env, "target_contract").into_val(env));
    data.push_back(target_contract.clone().into_val(env));
    data.push_back(Symbol::new(env, "success").into_val(env));
    data.push_back(success.into_val(env));
    data.push_back(Symbol::new(env, "operation").into_val(env));
    data.push_back(operation.clone().into_val(env));

    env.events().publish(topics, data);
}

/// Emit protocol configuration event
pub fn emit_protocol_config_event(
    env: &Env,
    protocol: &String,
    enabled: bool,
    admin: &Address,
) {
    let topics = (Symbol::new(env, "amm_protocol_config"), admin.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));
    data.push_back(Symbol::new(env, "enabled").into_val(env));
    data.push_back(enabled.into_val(env));
    data.push_back(Symbol::new(env, "admin").into_val(env));
    data.push_back(admin.clone().into_val(env));

    env.events().publish(topics, data);
}

/// Emit lending callback event
pub fn emit_lending_callback_event(
    env: &Env,
    user: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    amount_out: i128,
    operation: &String,
) {
    let topics = (Symbol::new(env, "amm_lending_callback"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_in").into_val(env));
    data.push_back(token_in.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_out").into_val(env));
    data.push_back(token_out.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_in").into_val(env));
    data.push_back(amount_in.into_val(env));
    data.push_back(Symbol::new(env, "amount_out").into_val(env));
    data.push_back(amount_out.into_val(env));
    data.push_back(Symbol::new(env, "operation").into_val(env));
    data.push_back(operation.clone().into_val(env));

    env.events().publish(topics, data);
}