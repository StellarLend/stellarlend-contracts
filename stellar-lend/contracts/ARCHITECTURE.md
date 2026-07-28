# StellarLend Contract Architecture

This note documents the contract boundaries under `stellar-lend/contracts/` and identifies which crates are canonical deployment targets.

## Canonical Deployment

`contracts/lending` is the canonical deployment target for the lending protocol.

`contracts/amm` is a separate optional deployment for AMM integration. It is not the source of truth for lending positions.

`contracts/hello-world` is legacy and not canonical for deployment. It remains in the active workspace at `stellar-lend/Cargo.toml` only so it compiles against the same shared Soroban SDK pin as the maintained contracts.

## Boundary Map

| Crate | Purpose | Canonical? |
| --- | --- | --- |
| `lending` | Core lending state machine and protocol controls | Yes |
| `amm` | Auxiliary AMM router/integration surface | Optional only |
| `hello-world` | Older all-in-one prototype with overlapping responsibilities | No |
| `bridge` | Bridge-specific contract | Separate concern |
| `common` | Shared library code | Library only |

## Workspace Dependency Pinning

The active Rust workspace is defined in `stellar-lend/Cargo.toml`. Soroban SDK
and other shared Rust dependencies must be pinned once under
`[workspace.dependencies]` and inherited by member crates with
`{ workspace = true }`.

Current workspace members:

- `contracts/common`
- `contracts/hello-world`
- `contracts/lending`
- `contracts/multisig`

Policy:

- do not pin `soroban-sdk` independently inside member crates
- do not introduce a second Soroban SDK version through direct crate-level version specs
- keep shared test dependencies, such as `proptest`, in the workspace dependency table when more than one member uses them
- treat `contracts/hello-world` as a compatibility consumer of the shared pin, not as the canonical deployment artifact

### Soroban SDK Version Bump Procedure

When upgrading Soroban-related dependencies:

1. Update the pinned versions in `stellar-lend/Cargo.toml` under `[workspace.dependencies]`.
2. Keep member crates on `workspace = true` instead of adding crate-local versions.
3. Refresh `Cargo.lock` from the workspace root and verify only the intended Soroban versions remain selected.
4. Run `cargo build --workspace` and `cargo test --workspace` from `stellar-lend/` before merging.

This keeps `Env`, `Address`, test utilities, and generated host types aligned
across all workspace members and makes upgrade review a single-file audit.

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

### AMM Initial Liquidity Minting

The `amm` implementation hardens pool initialization against "donation attacks" (where the first depositor inflates the share value to steal from subsequent depositors). On the first deposit (when `total_supply == 0`), the AMM mints `sqrt(amount_0 * amount_1)` shares, but subtracts and permanently locks a `MINIMUM_LIQUIDITY` amount (e.g., 1000 units) to a dead address. This enforces a minimum pool value and makes inflating the LP share price prohibitively expensive.

This invariant is enforced in the `add_liquidity` execution path, which issues the initial `MINIMUM_LIQUIDITY` shares to a dead address before returning the remaining `sqrt(x*y) - MINIMUM_LIQUIDITY` shares to the initial depositor. Subsequent deposits mint shares proportionally using `min(amount_0 * total_supply / reserve_0, amount_1 * total_supply / reserve_1)`.

It does not own lending solvency state, debt balances, or collateral balances.

### Constant-product invariant

The AMM implementation enforces a constant-product invariant guard on all
mutating operations (swaps and liquidity changes). Each path computes the
pool product `k = reserve_a * reserve_b` before and after the mutation and
asserts the expected monotonic direction: swaps and liquidity additions must
not decrease `k` (fees are retained in the pool), while liquidity removals
must not increase `k`. This check is implemented in the helper
`assert_k_monotonic` in the `contracts/amm` crate to detect rounding or
implementation errors and revert on violation.

### `hello-world`

`hello-world` combines lending, AMM, bridge, governance, analytics, and monitoring concerns in one crate. Because it remains in the active workspace only for compatibility and duplicates functionality now split across maintained crates, it should be treated as historical/reference code rather than the deployment artifact.

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

## AMM Swap: `min_out` and `deadline` Parameters

`AmmContract::swap` now requires two additional safety parameters that **must
be supplied by every caller**:

```
pub fn swap(
    env:       Env,
    caller:    Address,
    asset_in:  Address,
    asset_out: Address,
    amount_in: i128,
    min_out:   i128,   // NEW – minimum acceptable output (>= 0)
    deadline:  u64,    // NEW – latest valid ledger timestamp (seconds)
) -> Result<SwapResult, AmmError>
```

### Guard ordering (both checked before any state mutation)

| # | Condition | Error |
|---|-----------|-------|
| 1 | `env.ledger().timestamp() > deadline` | `DeadlineExpired` |
| 2 | `amount_out < min_out` | `SlippageExceeded` |

The deadline is checked **before** the slippage floor. This means a transaction
that is both stale and would produce sub-minimum output will surface
`DeadlineExpired`, not `SlippageExceeded`.

### Security rationale

- **`deadline`** rejects transactions that were signed for a different market
  state but sat in the mempool long enough for prices to move against the
  caller. Pass the current ledger timestamp plus a small grace window (e.g.,
  30–60 seconds / ledger-time equivalent).
- **`min_out`** provides a hard slippage floor that prevents sandwich attacks:
  a block-producer cannot insert trades that move the price against the caller
  beyond the declared tolerance.

### Caller migration guide

Callers that previously invoked `swap(caller, asset_in, asset_out, amount_in)`
must now supply `min_out` and `deadline`.

**Recommended derivation** (off-chain, before building the transaction):

```
// 1. Simulate the swap to get expected_out.
// 2. Apply your slippage tolerance (e.g. 50 bps = 0.5 %).
let min_out  = expected_out * (10_000 - slippage_bps) / 10_000;

// 3. Set deadline to now + grace_window (in ledger-time seconds).
let deadline = current_ledger_timestamp + 60;  // 60-second window
```

To **opt out** of a guard (not recommended in production):
- Pass `min_out = 0` to skip the slippage check.
- Pass `deadline = u64::MAX` to skip the deadline check.

**`scripts/init.sh` hint**: the `--amm-min-out-bps` flag (default `50`) is
printed during AMM initialisation as a documentation hint for the suggested
slippage tolerance. It is not stored on-chain.

### Error reference

| Error | Meaning |
|-------|---------|
| `AmmError::InvalidAmount (4)` | `amount_in <= 0` |
| `AmmError::InvalidMinOut (5)` | `min_out < 0` |
| `AmmError::DeadlineExpired (6)` | ledger timestamp > `deadline` |
| `AmmError::SlippageExceeded (7)` | `amount_out < min_out` |

## Recommendation

- deploy `contracts/lending` as the canonical lending contract
- deploy `contracts/amm` only as a separately reviewed auxiliary contract
- do not use `contracts/hello-world` as the canonical protocol deployment target
