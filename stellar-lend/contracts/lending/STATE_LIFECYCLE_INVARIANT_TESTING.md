# Stateful Lifecycle Invariant Testing

The lending contract's `deposit` / `withdraw` / `borrow` / `repay` /
`liquidate` entrypoints are individually covered by targeted unit tests, but
sequences of those operations — interleaved across multiple actors, with
interest accruing between operations — can violate global invariants that no
single-path test observes. This suite closes that gap with a **bounded,
property-based state-machine model** of the lifecycle.

Implementation: `src/stateful_lifecycle_invariant_test.rs`
(module `stateful_lifecycle_invariant_test`, wired into `src/lib.rs`).

---

## Design

### Approach: differential state-machine testing

The test generates a random sequence of operations (deposit, withdraw, borrow,
repay, liquidate, ledger-time advances, and governable-parameter changes),
replays it against a **fresh contract instance** in the Soroban test `Env`,
and maintains a **reference model** of the contract's entire observable state.
After *every* operation it asserts that the contract's on-chain state matches
the model exactly.

The model reuses the contract's own pure math functions from `debt.rs`
(`accrue_index`, `settle_position`, `effective_debt`) so that interest math is
not re-implemented (it is already pinned by the pure proptests
`compound_interest_proptest.rs`, `reserve_split_proptest.rs`,
`max_borrow_proptest.rs`, and `mul_div_proptest.rs`). What this suite adds is
coverage of the **state machine**: which storage keys are written, in what
order, on which success/failure branches, and whether the aggregate
accounting (total deposits, total debt, insurance fund, bad debt, borrow
index) stays consistent with the per-position state.

### Generated actors and assets

Each generated case spawns `NUM_USERS = 4` actors and a pair of mock tokens
(a debt asset and a collateral asset, `liquidate_transfer_test::MockToken`).
Any actor may act as depositor, borrower, repayer, or liquidator. Token
balances are minted far above what a bounded sequence can exhaust so
liquidation transfers never fail on balance grounds.

### State mirrored by the model

| Contract storage / view | Model field |
| --- | --- |
| `DataKey::Collateral(user)` | `ModelUser::collateral` |
| `DataKey::Debt(user)` | `ModelUser::position` (`DebtPosition`) |
| `DataKey::BorrowIndex` | `Model::index` |
| `DataKey::LastIndexUpdate` | `Model::last_index_update` |
| `DataKey::TotalDeposits` | `Model::total_deposits` |
| `DataKey::TotalDebt` | `Model::total_debt` |
| `DataKey::BadDebt` | `Model::bad_debt` |
| `DataKey::InsuranceFund` | `Model::insurance_fund` |
| `DataKey::InsuranceShareBps` | `Model::insurance_share_bps` |
| `DataKey::DepositCap` | `Model::deposit_cap` |
| `DataKey::DebtCeiling` | `Model::debt_ceiling` |
| `DataKey::BorrowMinAmount` | `Model::min_borrow` |
| `DataKey::CloseFactorBps` | `Model::close_factor_bps` |
| `DataKey::LiquidationIncentiveBps` | `Model::incentive_bps` |
| `DataKey::LiquidationThresholdBps` | `Model::threshold_bps` |
| Σ user collateral | `Model::sum_collateral` |
| `TotalDeposits` − Σ collateral (protocol reserve) | `Model::reserve` |

The governable parameters can be *changed mid-sequence* by generated admin
operations (`set_min_borrow`, `set_deposit_cap`, `set_debt_ceiling`,
`set_close_factor_bps`, `set_liquidation_incentive_bps`,
`set_liquidation_threshold_bps`, `set_insurance_share`), so gating behavior is
exercised across configurations rather than only at defaults.

### Determinism

* The suite uses a **fixed ChaCha seed** (`STATE_SEED`), so every CI run
  replays exactly the same generated sequences.
* The borrow rate is constant: rate params are never configured, so
  `current_borrow_rate` returns `debt::DEFAULT_APR_BPS` (500 bps) throughout.
  No oracle/valuation asset is configured, so price staleness gates are
  no-ops and the model is fully closed.
* Overrides for reproducibility and CI tuning (see below):
  * `STELLARLEND_STATE_CASES` — number of generated sequences (default 64).
  * `STELLARLEND_STATE_SEED` — 64-hex-digit seed (`0x…` optional) to replay a
    specific failure.

### Shrinking and failure reporting

Operation sequences are plain proptest strategies, so a failing case
**shrinks to a minimal sequence**. The `TestRunner` is configured with a
bounded `max_shrink_iters`; the test failure includes both the exact seed
and proptest's minimized sequence. Replay a failure with the seed printed in
that message:

```bash
STELLARLEND_STATE_SEED=0x<seed-from-failure> \
  cargo test -p stellarlend-lending --lib stateful_lifecycle
```

---

## Invariants asserted after every operation

1. **Debt invariants**
   - `get_position(user).debt >= 0` and equals the model's effective debt
     (`debt::effective_debt` — the same formula the view uses).
   - Stored `get_debt_position(user)` matches the model's `DebtPosition`
     field-for-field once a `Debt` entry exists; for never-touched users the
     fabricated default (`principal = 0`, `snapshot = INDEX_SCALE`,
     `last_update = read time`) is checked explicitly.
   - Protocol `total_borrow` equals the model's `TotalDebt` accumulator.
2. **Collateral invariants**
   - `get_position(user).collateral >= 0` and equals the model.
   - Protocol `total_supply` equals the model's `TotalDeposits`.
3. **Reserve / insurance / bad-debt invariants**
   - `TotalDeposits − Σ collateral == reserve` and `reserve >= 0`; the
     reserve grows only by liquidation seizures.
   - `get_bad_debt()` and `get_insurance_fund()` match the model; a
     liquidation shortfall draws insurance first, then books residual bad
     debt.
4. **Authorization invariants** (deterministic tests, real auth)
   - A user op without the position owner's auth is rejected with zero state
     change; an unrelated signer cannot withdraw another user's collateral.
   - A non-admin cannot change governed configuration (typed `Unauthorized`,
     nothing written).
   - Self-liquidation is rejected (`SelfLiquidation`) before any state
     change.
5. **Global accounting**
   - `get_borrow_index()` equals the model's index (monotonic).
   - Utilization (`total_borrow * 10000 / total_supply`) matches the model.
   - Every governable parameter reads back equal to the model.
6. **Failure semantics** — invalid operations return the *typed* error the
   contract defines (never a host trap), and the position and global
   accumulators are not partially mutated. The reference model snapshots its
   state before each mutating call and restores it when the invocation fails,
   matching Soroban's transaction-level rollback semantics.

---

## Observed implementation behaviour the model pins

The model mirrors today's implementation exactly, including behaviours that
differ from a naive "sum of positions" mental model. These are deliberate and
documented so a future behavioural change shows up as a test failure rather
than being silently absorbed:

* **`liquidate` does not update `TotalDebt`.** Repaid principal is removed
  from the borrower's position but not from the aggregate, so
  `TotalDebt` ≠ Σ principals after a liquidation. (The existing
  `rate_updated_event_test.rs` independently notes the aggregate is not
  maintained by every path.)
* **`repay` clamps the `TotalDebt` deduction.** The deduction is
  `prev_principal.checked_sub(updated_principal).unwrap_or(0)`; when the
  interest settled since the last touch exceeds the repayment amount, the
  deduction clamps to zero and the aggregate does not track the interest
  growth.
* **`liquidate` does not reduce `TotalDeposits` when collateral is seized.**
  Seized collateral leaves the borrower's balance while `TotalDeposits`
  stays put; the difference accumulates in the protocol reserve
  (`Model::reserve`), which is exactly the "`TotalDeposits` surplus" the
  governed `write_off_bad_debt` consumes.
* **Failed lifecycle calls are atomic.** Although `borrow`, `repay`, and
  `liquidate` perform settlement work before later validation, a returned
  error rolls back that work with the rest of the Soroban invocation. The
  model therefore commits settlement effects only after a successful call.
* **Borrow solvency uses the constant `LIQUIDATION_THRESHOLD_BPS` (8000)**,
  while liquidation eligibility and the `get_position` health-factor view use
  the *governable* threshold — an intentional asymmetry the model preserves.

The model asserts the contract-vs-model mirror for every one of these, so the
tests stay green on the current implementation while pinning the exact
accounting rules.

---

## CI runtime limits

The suite is deliberately bounded:

| Knob | Default | Notes |
| --- | --- | --- |
| `STATE_CASES` | 64 | Generated sequences per run |
| `MAX_OPS` | 48 | Operations per sequence |
| Amounts | 1…1,000 | Per operation |
| Time steps | 0…1 year | Per `AdvanceTime` |
| `MAX_SHRINK_ITERS` | 4,096 | Bound on shrinking work |

At these defaults the whole suite completes in seconds-to-low-minutes inside
the CI `build-and-test` job (same order as the existing
`property_invariants_test.rs` / `liquidation_sequence_invariant_test.rs`
suites, which run 128 × 64 and 48 × 16 cases respectively). CI can shrink the
budget without code changes:

```bash
STELLARLEND_STATE_CASES=16 cargo test -p stellarlend-lending --lib stateful_lifecycle
```

## Running

```bash
cd stellar-lend
cargo test -p stellarlend-lending --lib stateful_lifecycle
```

Coverage: the generated suite exercises every lifecycle entrypoint, both
success and typed-failure paths, multi-actor interleavings, interest
accrual windows, liquidation with and without shortfalls (insurance draw +
bad debt), and mid-sequence governance changes. Failed calls are checked as
atomic at the model boundary. The deterministic tests in
the same module pin authorization and no-partial-mutation behaviour without
randomness.

---

## Trade-offs and limitations

* **Single-asset accounting only.** The model covers the legacy
  single-asset lifecycle (`DataKey::Collateral(user)` /
  `DataKey::Debt(user)`). Cross-asset positions (`CollateralAsset` /
  `DebtAsset` with oracle prices, isolation mode, per-asset caps) are a
  separate, substantially larger state space and are explicitly out of
  scope; extending the model to it is the natural next step.
* **Shared math.** The model calls the same pure `debt` functions the
  contract uses. It therefore cannot detect a bug that is *also* in those
  functions — that is the job of the pure proptests, which remain the
  authority on the arithmetic.
* **`mock_all_auths` in the generated model.** The generated model uses
  `env.mock_all_auths()` (matching the other property suites in the crate);
  authorization is therefore verified by the deterministic real-auth tests
  in the same module rather than inside the generated sequences.
* **Constant borrow rate.** Rate params are not configured, so the rate is
  the constant 500 bps default. A follow-up could generate `RateParams`
  changes and model the smoothed rate, at the cost of replicating the rate
  model in the reference state.
* **No oracle, pause, emergency, or write-off operations.** The model keeps
  those subsystems out of scope; their targeted suites
  (`oracle_staleness_test.rs`, `emergency_state_matrix_test.rs`,
  `bad_debt_write_off_test.rs`) remain authoritative.
* **Bounded magnitudes.** Amounts and time steps are bounded so the model's
  arithmetic never overflows; the extreme-value behaviour is covered by the
  dedicated overflow/edge tests.

## Acceptance criteria mapping

| Criterion | Where it is met |
| --- | --- |
| Generated sequences preserve debt, collateral, reserve, authorization invariants | Invariants 1–5 above, checked after every operation |
| Invalid operations fail without partial mutation | Invariant 6 + `invalid_operations_fail_without_partial_mutation` |
| Failures provide a reproducible minimized seed and sequence | Fixed `STATE_SEED`, bounded proptest shrinking, failure output containing the seed and minimized input, and `STELLARLEND_STATE_SEED` replay |
| Suite runs reliably in CI, complements targeted tests | Bounded budget (table above); runs under the existing `cargo test --lib` step; no CI changes needed |
| Existing CI/CD checks remain green | No production code changed; only a new `#[cfg(test)]` module and its registration |
