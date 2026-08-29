# Security Notes & Trust Boundaries

## Trust Boundaries
- **Admins:** The highest level of privilege. Admins can update parameters (such as minimum borrow amounts, deposit ceilings, and oracles), pause the protocol, trigger emergency shutdown, and designate guardians. They are also responsible for upgrading the protocol.
- **Guardians:** Designed for rapid response. Guardians can only trigger emergency shutdowns. They cannot upgrade contracts, unpause the system, or change parameters.
- **Users:** End-users interact with the protocol via `deposit`, `borrow`, `repay`, and `withdraw` mechanisms subject to protocol checks. User operations are sandboxed to their respective `Address` scopes.
- **Oracles:** Trusted entities providing price feeds used for health factor checks. If an oracle becomes malicious, it could trigger improper liquidations, but internal checks restrict maximum liquidation amounts (via close factor limits).

## Authorization Model
All external entry points modifying state or user balances call `user.require_auth()`. This delegates authorization entirely to the Soroban SDK's robust authorization framework. 
Protocol functions restricted to Admins enforce validation via `admin.require_auth()` and ensure the caller matches the registered Admin in the data store.

## Reentrancy Protections
In Soroban, contract logic guarantees atomicity. However, as an added measure against logic-based reentrancy across cross-contract calls:
- All external calls to update state (e.g. `save_deposit_position`) occur *before* external token transfers where applicable (the Checks-Effects-Interactions pattern).
- High-risk operations are guarded by global pause mappings which an Admin or Guardian can engage via the pause module if anomalous behavior occurs.

## ⚠️ P0 Finding — Missing Token Custody on Core Position Operations

> **Correction notice:** A previous version of this document incorrectly
> asserted that *"All position operations (`deposit`, `borrow`, `repay`,
> `withdraw`) now explicitly enforce token transfers via the Soroban
> `token::Client`."* **That claim is factually wrong** for the base
> entrypoints in the canonical lending contract. It is recorded here as a
> finding so that auditors, integrators, and reviewers reading this file
> cannot miss it. Trusting the prior claim would lead a reviewer to
> believe user funds are safely custodied by the contract on deposit;
> they are not.

### What the base entrypoints actually do

The base `LendingContract` entrypoints in `src/lib.rs` are **accounting-only**:
they mutate protocol counters, but they perform **no token transfers**.

| Entrypoint (in `src/lib.rs`) | Real `TokenClient::transfer` / `transfer_from` call? | Behaviour |
|---|---|---|
| `deposit` (`src/lib.rs#L1118`) | **No.** | Increments `DataKey::Collateral(user)` and `TotalDeposits` via `checked_add`. No tokens are pulled. |
| `withdraw` (`src/lib.rs#L1162`) | **No.** | Decrements `DataKey::Collateral(user)` and `TotalDeposits` via `checked_sub`. No tokens are pushed. |
| `borrow` (`src/lib.rs#L1247`) | **No.** | Increments `Debt(user)` and `TotalDebt`. No tokens are pushed to the borrower. |
| `repay` (`src/lib.rs#L1809`) | **No.** | Decrements `Debt(user)` and `TotalDebt`. No tokens are pulled from the user. |
| `borrow_against_collateral` (`src/lib.rs#L1324`) | **No.** | Cross-asset isolation-aware variant of `borrow`; mutates `Debt(user)` / `TotalDebt` only. |
| `repay_against_collateral` (`src/lib.rs#L1392`) | **No.** | Cross-asset isolation-aware variant of `repay`; mutates `Debt(user)` / `TotalDebt` only. |
| `liquidate` (`src/lib.rs#L1524`) | **Yes** (only entry in the file). | Pulls debt-token from the liquidator into the contract, then pushes seized collateral to the liquidator. |
| `flash_loan` (`src/lib.rs#L1988`) | **No.** | Updates internal `Treasury` / receiver-balance state and invokes the receiver via `env.invoke_contract::<Val>(&receiver, &Symbol::new(&env, "on_flash_loan"), …)` (`L2043`–`L2045`). The actual token movement is delegated to the receiver, which is expected to use `TokenClient` to move tokens into and out of the lending contract; the lending contract itself performs no transfer. |

As of this revision, the only references to `TokenClient` in the entire
`src/lib.rs` file are the `use soroban_sdk::token::Client as TokenClient;`
import (`L147`) and the `TokenClient::new(...)` / `.transfer(...)` calls
inside `liquidate` (~`L1643`–`L1648`). Every other entrypoint listed
above — `deposit`, `withdraw`, `borrow`, `repay`,
`borrow_against_collateral`, `repay_against_collateral`, `flash_loan` —
neither constructs a `TokenClient` nor calls `.transfer(...)` or
`.transfer_from(...)` (verifiable via `rg 'TokenClient' stellar-lend/contracts/lending/src/lib.rs`
or `rg 'TokenClient|\\.transfer(_from)?\\(' stellar-lend/contracts/lending/src/lib.rs`).
If a future revision re-introduces `TokenClient::new` inside any of those
entrypoints, this section **must** be updated alongside the code change;
otherwise it silently rots.

### Why this is a P0 finding

- Calling `deposit(user, amount)` **credits the user's collateral
  counter** without taking custody of any underlying tokens from the user.
  The borrowing contract does not hold user funds on deposit.
- Calling `borrow(user, amount)` **increments the user's debt counter**
  without funding the user.
- Calling `repay(user, amount)` **decrements the user's debt counter**
  without pulling tokens from the user.
- Calling `withdraw(user, amount)` **decrements the user's collateral
  counter** without pushing tokens back.

Any integrator, front-end, or protocol-managed flow that treats the
resulting `Collateral(user) / Debt(user)` / `TotalDeposits / TotalDebt`
counters as denoting **custodied funds** — without separately moving the
matching tokens — permits users to mint **phantom collateral** and borrow
against unbacked positions. This drains protocol liquidity and other
depositors.

### Remediation paths required today

Until the canonical contract is extended to custody tokens on
`deposit` / `withdraw` / `borrow` / `repay`, **integrators must move
funds out-of-band** before or after calling the counter-mutating
entrypoint. Concretely:

1. **Deposits and repays:** route through the pull-based `receive`
   entrypoint documented in
   [`token_receiver.md`](./token_receiver.md), which calls
   `token::Client::transfer_from` on the user's approved allowance
   **before** applying the position update.
2. **Withdrawals:** push tokens to the user (or via a wrapper) before
   calling `withdraw`; the base `withdraw` does not move tokens.
3. **Borrows:** push borrowed funds to the user (or via a wrapper)
   before or after calling `borrow`; the base `borrow` does not move
   tokens.

See also:
- [`token_receiver.md`](./token_receiver.md) — pull-based `receive`
  entrypoint and missing-custody discussion.
- `src/lib.rs#L1524` (`liquidate`) — the **only** function in the
  file that performs real `TokenClient` transfers; useful as a
  reference for the canonical transfer framing this contract should
  eventually adopt for the four core position operations.

---

## Cross-Asset Module Hardening

> **⚠️ Retracted claim.** A previous version of this section listed *"Token
> Transfer Enforcement: All position operations (`deposit`, `borrow`,
> `repay`, `withdraw`) now explicitly enforce token transfers via the
> Soroban `token::Client`."* That bullet has been **removed** because it
> was factually incorrect — see the **⚠️ P0 Finding — Missing Token Custody
> on Core Position Operations** section above. The bullets below apply to
> **cross-asset** (`src/cross_asset.rs`) behaviours only and must not be
> conflated with the base `src/lib.rs` entrypoints.

- **Granular Pause Support:** Cross-asset operations now respect specific `PauseType` settings (e.g. `PauseType::Borrow`), allowing for targeted emergency interventions.
- **Event-Driven Transparency:** Each significant operation emits a unique contract event (`CrossDepositEvent`, etc.), facilitating robust off-chain monitoring and audit trails.
- **Initialization Safety:** The `initialize` function (which a previous revision of this document mis-named `initialize_admin`) returns `Result<(), LendingError>` and prevents re-initialization with a typed `LendingError::AlreadyInitialized` error if an admin is already set, rather than silently overwriting.

## Arithmetic Bounds
Protocol parameters strictly utilize `checked_add`, `checked_sub`, `checked_mul`, and `checked_div` to prevent overflow and underflow paths. Zero-amount and uninitialized parameter paths intentionally return structured `ContractError` values rather than panicking where possible.

## Withdraw path (`withdraw.rs`)
- **Pause module**: Withdraw is blocked when `pause::is_paused(Withdraw)` is true (this includes global `PauseType::All`), when the legacy `WithdrawDataKey::Paused` flag is set, or when the protocol is in **emergency shutdown** (`blocks_high_risk_ops` and not in **recovery**). In **recovery**, users may still withdraw (and repay) to unwind positions.
- **Collateral ratio**: Post-withdraw collateral must satisfy the same minimum ratio as borrows, via shared `borrow::validate_collateral_ratio` (150% default, `MIN_COLLATERAL_RATIO_BPS`).
- **Authorization**: Only the position owner can withdraw; `user.require_auth()` is enforced before state changes.

### Liquidation Boundary and Health Factor Scaling
The protocol represents the Health Factor using a scalar where `10_000` equates to `1.0`. 
To ensure determinism and avoid rounding ambiguity, the protocol strictly enforces the `<` threshold for liquidation eligibility. 
* A position with a Health Factor `<= 9_999` **is eligible** for liquidation.
* A position with a Health Factor `>= 10_000` **is completely immune** to liquidation. 

There are no edge cases where a `10_000` Health Factor allows for liquidation. All price oracle rounding uses integer truncations designed to safely error on the side of protecting the borrower from false-positive liquidations.

### Self-Liquidation Guard
The `liquidate` entry point now rejects any call where the liquidator address matches the borrower address. This guard triggers before any collateral or debt state reads, preventing a borrower from liquidating their own position to capture the liquidation incentive and profit from the protocol's close-factor mechanics.

## Oracle Migration Risks and Mitigation

Changing the protocol oracle (either the legacy address or the hardened module primary/fallback slots) is a high-risk administrative action that impacts the valuation of all open positions.

### Risks
- **Price Jump Liquidation**: Swapping to an oracle that reports a significantly lower price for collateral (or higher for debt) can instantly push healthy positions into liquidation eligibility.
- **Staleness Gaps**: If a new oracle has not yet submitted a price feed, valuation will fail (returning 0), blocking withdrawals and potentially enabling liquidations if not handled safely.
- **Misconfiguration**: Setting an incorrect oracle address or one with different decimal scales (the protocol expects 8 decimals) leads to incorrect health factor calculations.

### Mitigation and Operational Guidance
- **Deterministic Precedence**: The protocol prioritizes the Hardened Oracle Module over the Legacy Oracle address. This allows for a "staged" migration where a hardened feed is configured and verified before removing the legacy fallback.
- **Auditable Transitions**: All oracle changes emit events (`OracleSetEvent` or `OracleConfigEvent`) containing the admin, the new address, and the timestamp, ensuring a clear audit trail of price-source transitions.
- **Safe Failure Modes**: If an oracle returns an invalid price or is missing, the health factor defaults to 0. The `liquidate` function explicitly rejects positions with HF=0 to prevent "phantom liquidations" caused by missing price data.
- **Pre-Migration Valuation**: Admins should use view functions (`get_user_position`) with the proposed oracle price off-chain before committing the change on-chain to ensure no mass-liquidation event is triggered.

---

## Overflow and Underflow Protection (Integer Arithmetic Safety)

### Core Policy

All state-mutating operations (deposit, withdraw, borrow, repay) in the StellarLend lending contract use **checked arithmetic** (`i128::checked_add`, `i128::checked_sub`) to prevent integer overflow and underflow vulnerabilities. This is enforced independently of compiler flags via explicit error handling, providing defense-in-depth protection.

### Threat Model

In unprotected systems, integer overflow/underflow can cause:
- Silent balance wraparound (e.g., i128::MAX + 1 wraps to i128::MIN)
- Loss of user collateral or protocol insolvency  
- Broken accounting invariants that accumulate over time

### Protected Operations

**Deposit**: User collateral and protocol total deposits increased via `checked_add`
```rust
let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
let new_total = total_deposits.checked_add(amount).ok_or(LendingError::Overflow)?;
```

**Withdraw**: User collateral and protocol total deposits decreased via `checked_sub`
```rust
let new_balance = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
let new_total = total_deposits.checked_sub(amount).ok_or(LendingError::Overflow)?;
```

**Borrow**: User principal and protocol total debt increased via `checked_add`
```rust
let new_total = total_debt.checked_add(amount).ok_or(LendingError::Overflow)?;
```

**Repay**: User principal and protocol total debt decreased via `checked_sub`
```rust
let new_total = total_debt.checked_sub(amount).ok_or(LendingError::Overflow)?;
```

**Flash Loans**: Treasury and receiver balances transferred via `checked_add/checked_sub`, fee calculated with `checked_mul`

**Health Factor Calculation**: Collateral * 8000 (coefficient) computed with `checked_mul`, defaults to `i128::MAX` (safe) on overflow

### Error Propagation

All overflow conditions return `LendingError::Overflow` (error code 2003) consistently:
- Caller matches on `Err(LendingError::Overflow)` to reject transaction
- Error clearly distinguishes from other failure modes (cap exceeded, insufficient collateral, etc.)
- Enables robust monitoring and user-facing error messages

### Build Profile Independence

Cargo.toml enables `overflow-checks = true` for all profiles (debug, release, test) as a secondary defense. The primary defense is the explicit checked arithmetic in code:
- **Future-proof**: Changes to build settings cannot silently re-enable wraparound
- **Auditable**: Code review can verify all arithmetic uses checked variants
- **Testable**: Adversarial tests verify error returns, not just panic prevention

### Testing Verification

Adversarial test suite (minimum 95% coverage) validates:
- Deposit/borrow at i128::MAX / N for N = 2, 3, 4, 5...
- Repay/withdraw at extreme values without underflow
- Protocol-level total tracking with multiple users at near-max values
- Health factor calculation near i128::MAX without overflow

Example test: `test_deposit_at_max_balance_near_limit` deposits i128::MAX/2, then verifies second large deposit fails with Overflow error.

### Debt Module Consistency

The `debt.rs` module (interest accrual, principal mutations) follows the same checked arithmetic discipline:
- `settle_accrual()`: `checked_add` for interest + principal
- `effective_debt()`: `checked_add` for cumulative debt
- `borrow_amount()`: `checked_add` for new borrowing
- `repay_amount()`: `checked_sub` for repayment
- All return `Result<_, DebtError::Overflow>` on arithmetic failure

## Liquidation Rounding Policy

All three divisions in the `liquidate` path use **floor rounding** (truncation
toward zero for positive inputs) so that every sub-unit remainder favours
protocol solvency over the liquidator:

| Division | Rounding | Effect |
|---|---|---|
| `hf = collateral × 8000 ÷ debt` | floor | Lower HF → position appears more underwater → liquidation triggered sooner |
| `max_repay = debt × 5000 ÷ 10000` | floor | Smaller close-factor cap → less debt extinguished per liquidation → more rounds remain |
| `seized = repay × 11000 ÷ 10000` | floor | Liquidator receives *less* collateral than the exact 10 % bonus → remainder stays with borrower/protocol |

These are enforced via `math::checked_mul_div_floor`, which uses
`checked_mul` + `checked_div` with explicit floor semantics.  A companion
`math::checked_mul_div_ceil` exists for non-liquidation paths that need the
opposite direction, but it is **never** used in `liquidate`.

### Dust attack mitigation

A liquidator who repeatedly triggers small (dust) liquidations
can never accumulate a net positive due to rounding, because every truncation
transfers value *away* from the liquidator.  Concretely:

- If `seized_collateral` would be 1.1, the liquidator receives 1 and the 0.1
  stays with the borrower.
- If `max_repay` would be 0.5, the cap rounds to 0 (and the liquidation is
  rejected via the `actual_repay <= 0` dust guard), preventing a no-op call
  from wasting gas.

### Audit Checklist

- ✅ All core flows (deposit, withdraw, borrow, repay) use checked arithmetic
- ✅ LendingError::Overflow defined with unique error code (2003)
- ✅ Flash loan functions use checked_add/checked_sub/checked_mul
- ✅ Query functions (get_position) use checked_mul for health factor
- ✅ NatSpec documentation comments document overflow invariants per entrypoint
- ✅ Adversarial tests cover extreme values and overflow scenarios
- ✅ Test coverage ≥ 95% for core flows and error paths
- ✅ No silent wraparound in any build profile (checked_add/sub primary defense)
- ✅ Error messages explicit ("deposit: collateral overflow", "repay_flash_loan: treasury balance overflow")

### Related Documentation

- **Implementation**: [lib.rs - Core Flows](./src/lib.rs) (deposit, withdraw, borrow, repay functions)
- **Tests**: [lib.rs - Adversarial Tests](./src/lib.rs#L889) (test_deposit_at_max_balance_near_limit, etc.)
- **Debt Module**: [debt.rs](./src/debt.rs) - Interest accrual with checked arithmetic
- **Rounding Strategy**: [rounding_strategy.rs](./src/rounding_strategy.rs) - Pattern for checked operations


## Liquidation Invariant: Checked Subtraction

`liquidate` computes `new_debt = debt - actual_repay` and
`new_col = collateral - final_seized`.

Both operations use `checked_sub` returning `LendingError::Overflow` on
underflow. This turns a silent `saturating_sub` floor-to-zero into a loud
failure, surfacing any logic bug where the close-factor or seizure clamp is
incorrect.

### Why underflow is unreachable on valid inputs

| Variable | Clamp that prevents underflow |
|---|---|
| `actual_repay` | Clamped to `min(amount, debt * CLOSE_FACTOR / 10000)` ≤ `debt` |
| `final_seized` | Clamped to `min(seized_collateral, collateral)` ≤ `collateral` |

### Why checked_sub is still necessary

If the clamp arithmetic is ever changed incorrectly, `saturating_sub` would
silently write `0` to debt or collateral, masking the bug. `checked_sub`
causes the transaction to revert with `LendingError::Overflow`, making the
violation observable in tests and on-chain.

**Test coverage:** `src/liquidate_checked_sub_test.rs`
