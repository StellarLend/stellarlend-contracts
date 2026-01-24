# AMM Integration Contract

This contract provides Automated Market Maker (AMM) integration with hooks for automated swaps and liquidity operations within the StellarLend protocol.

## Features

### Core AMM Operations
- **Token Swaps**: Execute swaps through multiple AMM protocols with slippage protection
- **Liquidity Management**: Add and remove liquidity from AMM pools
- **Multi-Protocol Support**: Supports Stellar DEX, Soroswap, and Phoenix protocols
- **Callback Validation**: Secure callback system for AMM operations

### Lending Integration Hooks
- **Pre/Post Operation Hooks**: Execute custom logic before and after AMM operations
- **Automated Liquidation**: Integration with lending protocol for automated liquidations
- **Collateral Management**: Automatic collateral rebalancing through AMM swaps
- **Yield Optimization**: Automated liquidity provision for yield generation

### Security Features
- **Slippage Protection**: Configurable slippage tolerance for all operations
- **Pause Mechanisms**: Emergency pause functionality for all operations
- **Callback Validation**: Secure callback system with nonce and timestamp validation
- **Overflow Protection**: Safe arithmetic operations throughout

## Contract Interface

### Initialization
```rust
pub fn initialize(env: Env, admin: Address) -> String
```

### Swap Operations
```rust
pub fn swap_with_hooks(
    env: Env,
    user: Address,
    token_in: Address,
    token_out: Address,
    amount_in: i128,
    min_amount_out: i128,
    amm_protocol: String,
    callback_data: Option<CallbackData>,
) -> i128
```

### Liquidity Operations
```rust
pub fn add_liquidity_with_hooks(
    env: Env,
    user: Address,
    token_a: Address,
    token_b: Address,
    amount_a: i128,
    amount_b: i128,
    min_liquidity: i128,
    amm_protocol: String,
) -> i128

pub fn remove_liquidity_with_hooks(
    env: Env,
    user: Address,
    token_a: Address,
    token_b: Address,
    liquidity_amount: i128,
    min_amount_a: i128,
    min_amount_b: i128,
    amm_protocol: String,
) -> (i128, i128)
```

### Validation
```rust
pub fn validate_amm_callback(
    env: Env,
    caller: Address,
    callback_data: CallbackData,
) -> bool
```

## Supported AMM Protocols

1. **Stellar DEX**: Native Stellar decentralized exchange
2. **Soroswap**: Uniswap V2-style AMM on Stellar
3. **Phoenix**: Advanced AMM with concentrated liquidity

## Events

The contract emits the following events:

- `amm_swap`: Token swap executed
- `amm_liquidity_added`: Liquidity added to pool
- `amm_liquidity_removed`: Liquidity removed from pool
- `amm_callback_validation`: Callback validation result
- `amm_hook_execution`: Hook execution result
- `amm_protocol_config`: Protocol configuration change

## Error Handling

The contract includes comprehensive error handling:

- `InvalidAmount`: Invalid swap or liquidity amount
- `InvalidToken`: Invalid token address
- `InsufficientLiquidity`: Insufficient pool liquidity
- `SlippageExceeded`: Slippage tolerance exceeded
- `UnsupportedProtocol`: AMM protocol not supported
- `InvalidCallback`: Invalid callback data
- `UnauthorizedCaller`: Unauthorized callback caller
- `PoolNotFound`: AMM pool does not exist
- `OperationPaused`: Operation is paused
- `HookFailed`: Hook execution failed

## Usage Examples

### Basic Token Swap
```rust
let amount_out = client.swap_with_hooks(
    &user,
    &token_usdc,
    &token_xlm,
    &1000, // 1000 USDC
    &950,  // Min 950 XLM
    &String::from_str(&env, "stellar_dex"),
    &None,
);
```

### Add Liquidity
```rust
let liquidity_tokens = client.add_liquidity_with_hooks(
    &user,
    &token_usdc,
    &token_xlm,
    &1000, // 1000 USDC
    &2000, // 2000 XLM
    &1800, // Min 1800 LP tokens
    &String::from_str(&env, "soroswap"),
);
```

### Swap with Callback
```rust
let callback_data = CallbackData {
    operation: Symbol::new(&env, "liquidation"),
    user: user.clone(),
    tokens: vec![token_in, token_out],
    amounts: vec![amount_in],
    metadata: Map::new(&env),
    timestamp: env.ledger().timestamp(),
    nonce: 1,
};

let amount_out = client.swap_with_hooks(
    &user,
    &token_in,
    &token_out,
    &amount_in,
    &min_amount_out,
    &protocol,
    &Some(callback_data),
);
```

## Testing

Run the test suite:
```bash
make test
```

The contract includes comprehensive tests covering:
- Basic swap operations
- Liquidity management
- Error conditions
- Edge cases
- Callback validation
- Multi-user scenarios
- Analytics tracking

## Security Considerations

1. **Slippage Protection**: Always set appropriate minimum output amounts
2. **Callback Security**: Validate all callback data and callers
3. **Pause Mechanisms**: Use pause functionality during emergencies
4. **Protocol Validation**: Only use supported and configured AMM protocols
5. **Amount Validation**: Ensure all amounts are positive and within limits

## Integration with StellarLend

The AMM contract integrates with the main StellarLend protocol through:

1. **Liquidation Hooks**: Automatic liquidation of undercollateralized positions
2. **Collateral Rebalancing**: Automatic rebalancing of collateral portfolios
3. **Yield Generation**: Automated liquidity provision for additional yield
4. **Flash Loans**: Integration with flash loan functionality for arbitrage

## Build and Deploy

```bash
# Build the contract
make build

# Run tests
make test

# Check code formatting
make fmt

# Run clippy lints
make clippy
```