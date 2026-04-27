# StellarLend AMM Integration Contract

This contract provides Automated Market Maker (AMM) integration for the StellarLend protocol, enabling automated swaps and liquidity operations within lending operations.

## Features

- **Multi-Protocol Support**: Integrates with multiple AMM protocols
- **Automated Swaps**: Execute token swaps with slippage protection
- **Liquidity Operations**: Add/remove liquidity from AMM pools
- **Callback Validation**: Secure callback handling with replay protection
- **Event Emission**: Comprehensive event logging for all operations
- **Collateral Optimization**: Auto-swap functionality for optimal collateral ratios

## Key Functions

### Admin Functions
- `initialize_amm_settings`: Set up AMM parameters
- `add_amm_protocol`: Register new AMM protocols
- `update_amm_settings`: Modify AMM settings

### User Functions
- `execute_swap`: Perform token swaps
- `add_liquidity`: Add liquidity to pools
- `remove_liquidity`: Remove liquidity from pools
- `auto_swap_for_collateral`: Optimize collateral ratios

### Protocol Functions
- `validate_amm_callback`: Validate AMM protocol callbacks

## Security Features

- Slippage protection with configurable tolerances
- Callback validation with nonce-based replay protection and deadline (expiry) checks
- Admin-only configuration functions
- Comprehensive parameter validation
- Emergency pause functionality integration
- Authorization checks (`require_auth`) on admin/user/protocol entrypoints

## Liquidity Share Math and Rounding

- Initial LP minting uses `floor(sqrt(amount_a * amount_b))`.
- Subsequent LP minting uses `floor(min(amount_a * total_lp / reserve_a, amount_b * total_lp / reserve_b))`.
- LP burns return `floor(lp_burned * reserve / total_lp)` per token.
- All rounding is floor-biased to preserve solvency and prevent over-credit/over-withdraw.

## Trust Boundaries

- **Admin authority**: can initialize and update AMM settings, and register protocol configs.
- **User authority**: users must authorize their own swap/add/remove operations.
- **Protocol callbacks**: registered AMM protocol addresses must authorize callback validation calls.
- **Token transfer flows**: this integration tracks AMM routing/LP accounting and callback safety; actual token transfer semantics depend on the integrated AMM/token contracts.

## Events

- `swap_executed`: Token swap details
- `liquidity_added`: Liquidity addition events
- `liquidity_removed`: Liquidity removal events
- `amm_operation`: General AMM operation tracking
- `callback_validated`: Callback validation events

## Usage

The AMM contract is designed to work seamlessly with the main StellarLend lending protocol, providing automated market making capabilities for optimal capital efficiency.