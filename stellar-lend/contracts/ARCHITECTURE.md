# StellarLend Contract Architecture

This note documents the contract boundaries under `stellar-lend/contracts/` and identifies which crates are canonical deployment targets.

## Canonical Deployment

`contracts/lending` is the canonical deployment target for the lending protocol.

`contracts/amm` is a separate optional deployment for AMM integration. It is not the source of truth for lending positions.

`contracts/hello-world` is legacy and not canonical for deployment. It is not included in the active workspace at `stellar-lend/Cargo.toml`, while `contracts/lending`, `contracts/amm`, `contracts/bridge`, and `contracts/common` are.

## Boundary Map

| Crate | Purpose | Canonical? |
| --- | --- | --- |
| `lending` | Core lending state machine and protocol controls | Yes |
| `amm` | Auxiliary AMM router/integration surface | Optional only |
| `hello-world` | Older all-in-one prototype with overlapping responsibilities | No |
| `bridge` | Bridge-specific contract | Separate concern |
| `common` | Shared library code | Library only |

## Ownership Boundaries

### `lending`

`lending` owns:

- collateral and debt state
- interest accrual and liquidation checks
- pause and emergency lifecycle
- admin and guardian controls
- token receiver flows for deposit and repay
- flash-loan entrypoints
- upgrade and data-store management helpers

This is the contract users, liquidators, and token contracts should treat as authoritative for lending state.

### `amm`

`amm` owns:

- AMM protocol registry
- swap and liquidity settings
- callback nonce tracking
- AMM operation history
- its own upgrade state

It does not own lending solvency state, debt balances, or collateral balances.

### `hello-world`

`hello-world` combines lending, AMM, bridge, governance, analytics, and monitoring concerns in one crate. Because it is outside the active workspace and duplicates functionality now split across maintained crates, it should be treated as historical/reference code rather than the deployment artifact.

## Trust Boundaries

### Lending trust assumptions

`lending` trusts:

- the configured admin for protocol parameter changes
- the configured guardian only for emergency shutdown initiation
- the configured oracle for read-only price queries
- Soroban token contracts that invoke the receiver hook correctly
- flash-loan receivers to return principal plus fee by the end of the callback transaction

`lending` does not trust arbitrary users to bypass auth, pause, or recovery gates.

### AMM trust assumptions

`amm` trusts:

- registered AMM protocol addresses
- callback identity for registered protocols
- admin-configured slippage and protocol settings

`amm` should not be treated as the canonical lending state machine.

## Admin And Guardian Powers

### Lending admin

The `lending` admin can initialize the contract, set pause flags, set the guardian, set the oracle, set liquidation parameters, set flash-loan fees, initialize operation settings, manage recovery, and operate the upgrade/data-store helpers.

### Lending guardian

The guardian can trigger `emergency_shutdown` and nothing else. The guardian cannot modify parameters, move user balances directly, or complete recovery.

### AMM admin

The `amm` admin is intended to initialize AMM settings, register protocols, update settings, and manage upgrades.

Security caveat: the local `amm` helper `require_admin` checks equality against stored admin state but does not call `require_auth()`. That means `amm` should be treated as an auxiliary contract pending auth hardening.

## Token Transfer Flows

### Lending deposit and repay

`lending::receive(token_asset, from, amount, payload)` dispatches only two actions:

- `deposit`
- `repay`

Unknown actions are rejected. Safety here relies on the Soroban token hook calling convention and trusted token contracts, because the hook accepts the `token_asset` argument as provided and does not independently verify that the invoker matches that token address.

### Lending flash loan

The flash-loan flow is:

1. read the lending contract token balance
2. transfer tokens to the receiver
3. invoke `on_flash_loan` on the receiver
4. read the final token balance
5. require repayment of principal plus fee

This is the main external call path in `lending`.

### AMM

Current `amm` swap and liquidity execution paths are still modeled with mock protocol helpers. They track slippage, callback validation, and history, but they are not yet a fully hardened production router.

## External Call Review

### `lending`

Reviewed external call surfaces:

- flash-loan token transfer
- flash-loan receiver callback
- oracle `price` queries in views
- token receiver hook entrypoint

Findings:

- user-sensitive entrypoints consistently require auth
- admin setters use explicit auth checks
- pause and recovery gates are checked on high-risk paths
- arithmetic mostly uses `checked_*` or widened math via `I256`
- flash loans have a dedicated reentrancy guard and repayment check

Caveat:

- `flash_loan::calculate_fee` uses saturating arithmetic instead of returning an explicit overflow error

### `amm`

Reviewed external call surfaces:

- callback validation
- swap/liquidity execution helpers
- admin mutation paths

Findings:

- slippage and amount bounds are validated
- callback nonces provide replay resistance
- arithmetic generally uses checked operations

Caveats:

- admin authorization is incomplete because `require_auth()` is missing in `require_admin`
- swap/liquidity execution is still mock-style rather than full external protocol integration

## Reentrancy Notes

`lending` has a dedicated flash-loan reentrancy guard. No general contract-wide guard exists, so the flash-loan callback remains the most important reviewed reentrancy surface.

`amm` has no explicit global reentrancy guard today. If it graduates from mocked integration to real external AMM calls, it should add one before production deployment.

## Parameter Bounds And Arithmetic

Important existing bounds include:

- `lending` close factor: `1..=10000`
- `lending` liquidation incentive: `0..=10000`
- `lending` flash-loan fee: `0..=1000`
- `amm` slippage bounded by configured `max_slippage`
- positive-amount checks on swap and liquidity inputs

## Recommendation

- deploy `contracts/lending` as the canonical lending contract
- deploy `contracts/amm` only as a separately reviewed auxiliary contract
- do not use `contracts/hello-world` as the canonical protocol deployment target
