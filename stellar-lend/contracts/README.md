# StellarLend Contracts Architecture

This note defines the on-chain contract boundary for the Rust crates under `stellar-lend/contracts/`.
It is the source of truth for deciding which crate is deployed as the protocol, which crates are optional companions, and which crates are legacy or development-only.

## Canonical Deployment Decision

`contracts/lending` is the canonical deployment crate for the lending protocol.

Why this is the source of truth:
- `stellar-lend/Cargo.toml` includes `contracts/lending`, `contracts/amm`, `contracts/bridge`, and `contracts/common` in the active workspace, but does not include `contracts/hello-world`.
- `contracts/lending` has the current production-focused surface for lending, pause, recovery, flash loans, views, token receiver hooks, upgrades, and data store helpers.
- `contracts/hello-world` is a legacy monolith with a much larger mixed surface area and is not built by the current workspace-level `cargo test` or `stellar contract build` flow.

Operationally:
- Deploy `stellarlend-lending` for the lending market.
- Deploy `stellarlend-amm` only if AMM routing/liquidity features are intentionally enabled and separately reviewed.
- Do not treat `hello-world` as the production deployment target.

## Boundary Map

| Crate | Role | Deployment status | Notes |
| --- | --- | --- | --- |
| `lending` | Canonical lending contract | Deploy | Core protocol state, admin/guardian controls, pause/recovery lifecycle, flash loans, upgrade hooks, views |
| `amm` | Optional companion contract | Deploy only with explicit review | Separate AMM router/integration surface; not the canonical lending market |
| `hello-world` | Legacy monolith / experimental superset | Do not deploy as canonical protocol | Not in workspace; broader API than current production boundary |
| `bridge` | Separate helper contract | Out of scope for this note | Not part of the hello-world vs lending vs amm decision |
| `common` | Shared library code | Not deployable by itself | Upgrade utilities shared by deployable crates |

## Recommended Deployment Topology

```text
Users / bots
    |
    v
+-----------------------+
| stellarlend-lending   |  canonical protocol state
| - debt/collateral     |
| - pause/recovery      |
| - flash loans         |
| - views/oracle reads  |
+-----------------------+
    |                \
    | optional        \ read-only price query
    v                  v
+------------------+  +------------------+
| stellarlend-amm  |  | oracle contract  |
| swap/liquidity   |  | price(asset)     |
+------------------+  +------------------+
```

## Crate-by-Crate Intent

### `contracts/lending`

This crate is the deployable protocol core.
Its external surface is intentionally narrower than `hello-world` and centers on:
- initialization and admin controls
- collateral, borrow, repay, withdraw, and liquidation flows
- emergency pause, shutdown, recovery, and guardian support
- flash loans
- read-only views over collateral, debt, and health factor
- upgrade and data-store helpers

This is the contract that downstream deployment, monitoring, and review should target first.

### `contracts/amm`

This crate is a separate AMM integration router, not the lending system of record.
It should be treated as an optional sidecar contract.
It may be deployed when swaps/liquidity are needed, but it should not replace the lending contract as the main protocol endpoint.

Important current boundary:
- AMM state is independent from lending state.
- Lending can exist without AMM.
- AMM failures should not redefine lending balances or admin state.

### `contracts/hello-world`

This crate is best understood as a legacy monolithic contract that aggregates many features into one surface:
- lending and cross-asset logic
- governance and multisig
- oracle and monitoring helpers
- AMM and bridge adapters
- recovery and upgrade flows

It is not the canonical deployment target because:
- it is not part of the active workspace members,
- its API is much wider than the current deployable boundary,
- it mixes concerns that are now split across focused crates.

Use it only as historical/reference code unless the team explicitly decides to revive and harden it.

## Trust Boundaries

### Users

Users are trusted only for their own authorization.
They are not trusted for pricing, admin actions, or callback integrity.
Every state-changing user path should require the user signer either directly or through a trusted token callback path.

### Admin

In `lending`, the admin is the highest-privilege operational role.
Admin powers include:
- initialize protocol settings
- pause/unpause operations
- set guardian
- set oracle
- tune liquidation threshold, close factor, liquidation incentive, deposit/withdraw settings, and flash-loan fee
- manage upgrade and data-store administration

Security assumption:
- the admin must be a multisig or similarly controlled governance address before mainnet use.

### Guardian

In `lending`, the guardian is intentionally narrower than admin.
Guardian power is limited to emergency shutdown initiation.
The guardian does not control normal parameter changes or recovery completion.

Security assumption:
- guardian keys must be fast-response emergency keys, not day-to-day operator keys.

### Oracle

`lending` view functions trust the configured oracle contract for pricing via external `price(asset)` calls.
That means health factor, collateral value, debt value, and liquidation calculations are only as sound as the admin-configured oracle.

Security assumption:
- oracle contract correctness and governance are outside this contracts note, but the oracle address must be treated as a critical trust dependency.

### AMM Protocols

`amm` trusts registered AMM protocol addresses for swap/liquidity callbacks after protocol registration and nonce checks.
That is a different trust boundary from `lending`.
AMM protocol registration should be reviewed as carefully as adding a new privileged integration.

## Token Transfer Flows

### `lending`

`lending` has two distinct classes of flows:

1. Accounting-only lending paths
- `deposit`, `borrow`, `repay`, and `withdraw` mostly update internal storage.
- These paths do not themselves perform SEP-41 token transfers.
- They should therefore be paired with explicit token plumbing at the integration layer.

2. External token/cross-contract paths
- `receive(...)` is the token receiver hook and routes incoming token callbacks into internal `deposit` or `repay` logic.
- `flash_loan(...)` transfers tokens to the receiver, calls the receiver callback, then verifies repayment by balance check.
- view functions call the oracle contract read-only for pricing.

Implication:
- `lending` is the canonical contract, but integrators must understand that some economic actions are accounting state transitions unless invoked through the expected token-receiver flow.

### `amm`

`amm` delegates swaps/liquidity operations to external AMM protocol contracts and validates callbacks with per-user nonces.
The AMM contract is therefore an integration router, not an isolated ledger.

### `hello-world`

`hello-world` contains direct token transfer and transfer-from logic across more modules than `lending`.
That larger transfer surface is one reason it should not be the default deployment target without a separate full review.

## External Call Path Review

### `lending`

Reviewed external paths:
- `flash_loan` -> token transfer to receiver -> `on_flash_loan` callback -> repayment balance check
- views -> oracle `price(asset)` call
- token receiver `receive` -> internal deposit/repay dispatch

Authorization and reentrancy notes:
- `flash_loan` uses an instance-storage reentrancy guard and validates post-callback repayment before success.
- admin-mutating entrypoints in `lending` consistently compare against stored admin and call `require_auth()`.
- guardian-triggered shutdown still requires signer auth before shutdown executes.
- view/oracle calls are read-only but trust the configured oracle.
- `receive` dispatches by payload symbol and assumes the surrounding token-callback environment is trusted; it does not itself prove that the caller equals `token_asset`.

Arithmetic and bounds notes:
- most lending parameter setters use explicit bounds for liquidation threshold, close factor, liquidation incentive, and flash-loan fee.
- core lending math generally uses `checked_add`, `checked_sub`, and `I256` for value computations.
- one notable exception is flash-loan fee calculation, which currently uses saturating arithmetic rather than checked arithmetic.

### `amm`

Reviewed external paths:
- swap/liquidity functions -> delegated AMM protocol execution
- callback validation via `validate_amm_callback`

Authorization and reentrancy notes:
- callback validation checks protocol registration and per-user nonce progression.
- however, current AMM entrypoints do not consistently require signer auth for the `user`/`admin` addresses they accept.
- `require_admin` checks stored admin equality but does not call `require_auth()`.
- `initialize_amm_settings` currently sets admin and settings without an auth gate and without explicit parameter bounds.

Security conclusion:
- `amm` should be treated as an optional companion contract that requires separate hardening review before production deployment.
- it is not an acceptable substitute for the canonical lending deployment boundary.

### `hello-world`

`hello-world` has a much larger set of direct token transfers, governance-driven cross-contract calls, and mixed concerns.
Given that surface area and its non-workspace status, it should be considered non-canonical until re-scoped and re-reviewed.

## Parameter Safety Expectations

For production-bound contracts under this directory, prefer:
- checked arithmetic over saturating arithmetic for protocol balances, fees, and risk calculations
- explicit min/max bounds on every externally settable protocol parameter
- signer verification on every state-changing path that accepts a caller/user/admin address
- narrow contracts with single-purpose trust boundaries rather than one monolith with overlapping roles

Applied to the current crates:
- `lending` mostly matches this direction and is the best deployment candidate.
- `amm` does not yet fully meet this bar because several privileged or user-scoped paths are missing explicit auth and tighter parameter validation.
- `hello-world` exceeds the desired trusted surface and should remain out of the canonical deploy path.

## Deployment Guidance

Use this rule set when deciding what to deploy:
- Deploy `lending` as the protocol contract.
- Deploy `amm` only when its feature set is needed and only after a dedicated auth/bounds review.
- Do not deploy `hello-world` as the main market contract.

If documentation elsewhere still points to `hello_world.wasm`, treat that as stale documentation and not as the current contracts source of truth.
