# Lending Lifecycle State Transitions — Invariants, Bounds, Diagnostics

Implementation: [`src/lifecycle.rs`](src/lifecycle.rs)
Tests: [`src/lifecycle_test.rs`](src/lifecycle_test.rs)
Entrypoints: `get_lifecycle_diagnostics`, `get_lifecycle_records`,
`simulate_lifecycle_transition` in [`src/lib.rs`](src/lib.rs)

Deposit, withdraw, borrow, repay and liquidate are the five transitions that
move value between a borrower's position and the protocol treasury. Before this
change the rules governing them were spread across the entrypoints, so each one
could drift from the others, and a degraded protocol (rejection storms, clients
stuck retrying, slow settlement) was only visible by scraping events off-chain.

This module states the rules once, as a pure function, and pairs them with a
**bounded** diagnostics ring so degraded behaviour is observable on-chain
without unbounded storage, unbounded reads, or disclosure of borrower identity.

---

## 1. Invariants

`lifecycle::evaluate` is the single guard. It is pure — no storage, no logging,
no panics — so a pre-flight simulation and the real write path cannot disagree.

### State (`S`)

| ID | Invariant |
|----|-----------|
| `S1` | A position is always well formed: `collateral >= 0 && debt >= 0`. Checked on the input *and* on the produced snapshot. |
| `S2` | `Withdraw` may not drive collateral below zero. |
| `S3` | `Repay` may not drive debt below zero. Overpayment is **rejected, not clamped**, so the caller learns the exact settlement amount instead of over-transferring. |
| `S4` | `Liquidate` requires both `debt > 0` and `collateral > 0`. |
| `S5` | A rejected transition is a no-op: the caller receives a reason and the unmodified snapshot. |

### Data (`D`)

| ID | Invariant |
|----|-----------|
| `D1` | `amount > 0`. Zero-amount calls are rejected rather than accepted as no-ops, so telemetry never counts phantom activity. |
| `D2` | `amount <= MAX_TRANSITION_AMOUNT` (`i128::MAX / 4`). |
| `D3` | Additive updates use `checked_add`; overflow is a rejection, never a wrap. |
| `D4` | Conservation: the applied delta equals exactly the requested amount on the affected leg, and the untouched leg is bit-identical. `verify_post` re-checks this against the *reloaded* position after a write. |

### Authorization (`A`)

| ID | Invariant |
|----|-----------|
| `A1` | Every transition requires an authorized caller. |
| `A2` | `Deposit`, `Withdraw`, `Borrow`, `Repay` are owner-only. |
| `A3` | `Liquidate` is third-party only — an owner may not liquidate their own position. |

Authorization is settled **before** any accounting. An unauthorized caller
asking to withdraw more than exists sees `Unauthorized`, never
`InsufficientCollateral`, so the error code cannot be used as a balance oracle.
This ordering is pinned by
`authorization_is_checked_before_accounting_so_balances_do_not_leak`.

### Failure (`F`)

| ID | Invariant |
|----|-----------|
| `F1` | Every rejection is classified (`Validation` / `Authorization` / `Accounting` / `Throttle` / `Internal`) so dashboards can separate user error from protocol degradation. `Internal` is always operator-actionable. |
| `F2` | Reason codes are stable and non-overlapping (enforced by test), and carry no address, balance or price — safe to surface verbatim in client telemetry. |
| `F3` | Consecutive rejections by one actor are counted and escalate at `MAX_RETRY_ATTEMPTS`, so a client stuck in a retry loop is visible without log scraping. |

---

## 2. Explicit bounds

Every budget is a compile-time constant, so the worst case is auditable, and
every one of them is echoed back in `get_lifecycle_diagnostics` — a dashboard
renders "how close are we to the cap" without hard-coding the values.

| Concern | Constant | Value |
|---|---|---|
| Memory / stored history | `MAX_LIFECYCLE_RECORDS` | 64 records, oldest evicted first |
| Pagination / read fan-out | `MAX_LIFECYCLE_PAGE` | 16 records per call |
| Concurrent writes per actor | `MAX_TRANSITIONS_PER_LEDGER` | 8 per ledger |
| Value magnitude | `MAX_TRANSITION_AMOUNT` | `i128::MAX / 4` |
| Retry budget before escalation | `MAX_RETRY_ATTEMPTS` | 3 |
| Latency series points | `MAX_LATENCY_BUCKETS` | 4 (edges 5s / 30s / 300s + overflow) |

`get_lifecycle_records(offset, limit)` **clamps** an oversized `limit` rather
than rejecting it, so a naive client that asks for everything still receives a
bounded response. An `offset` at or past the end returns an empty page — the
natural termination condition for a paging client.

---

## 3. Redundant work avoidance

Rapid interaction — a double-submit, a reconnecting client re-sending, an
indexer replaying — must not multiply storage writes:

* **Same-ledger duplicate folding.** A record identical to the newest one
  within the same ledger increments that record's `repeat_count` instead of
  appending. Three identical submissions produce one record and one
  `deduplicated` counter bump, not three entries.
* **Write-if-changed.** Counters and per-actor windows are compared against
  what is already stored and only written back when a field actually changed,
  turning no-op updates into pure reads.
* **Per-ledger budget.** Beyond `MAX_TRANSITIONS_PER_LEDGER` an actor's
  attempts are reported as `Throttled` and cost one counter bump rather than a
  history append. The budget resets on the next ledger; one actor's budget
  never throttles another's.
* **Pre-flight.** `simulate_lifecycle_transition` lets a client skip
  submitting a transaction that is already known to fail.

---

## 4. Diagnostics without secrets

`get_lifecycle_diagnostics` returns:

* `attempted` / `committed` / `rejected` / `throttled` / `deduplicated`
* `recovered` — commits that directly followed a rejection by the same actor
* `escalated` — rejection streaks at or beyond the retry budget
* `last_failure_class` / `last_failure_reason` / `last_failure_ledger`
* `latency_buckets` (4-point histogram) and `max_latency_secs`
* the six bounds above

`get_lifecycle_records` returns the bounded ring, newest first, each entry
carrying action, outcome, reason code, amount, ledger, timestamp,
inter-arrival latency and repeat count.

**No address appears anywhere.** Callers are identified only by
`actor_tag: u32` — the leading four bytes of `sha256(address_xdr)`. It is stable
enough to correlate one session's attempts and non-reversible, so reading the
history never discloses which account transacted. The counters carry no amounts
at all; the ring carries only the requested amount, which is already public via
the paired lifecycle event.

Latency is measured as the inter-arrival delta between one actor's consecutive
attempts. A replayed or reordered ledger with an earlier clock yields `0`
rather than a wrapped value.

---

## 5. Design tradeoffs

* **Reject overpayment instead of clamping it.** Clamping is friendlier but
  hides the true settlement amount from the caller and makes `D4` conservation
  unprovable. Rejecting costs an extra client round-trip and buys an exact,
  checkable delta.
* **Guard is pure; storage lives outside it.** This is what makes simulation
  and execution provably identical, and it keeps the whole invariant suite
  testable without a ledger. The cost is that callers must feed it a snapshot
  rather than the guard reading storage itself.
* **Throttling bounds the ring, never the money path.** A throttled attempt
  still commits; only its diagnostics record is dropped. Letting telemetry
  pressure fail a valid transition would be strictly worse than losing an
  observation, so `guard()` returns the verdict unchanged.
* **`i128::MAX / 4` rather than `i128::MAX`.** Leaves headroom for a
  bps-scaled product plus a paired addition downstream without every call site
  re-deriving its own bound.
* **64-record ring, 16-record page.** Sized for "what happened in the last few
  minutes", not for audit history — indexers consuming the lifecycle events
  remain the system of record. This keeps the storage footprint O(1).
* **Truncated 32-bit actor tag.** Collisions are possible at ~2^16 distinct
  actors (birthday bound). Accepted: the tag is a correlation hint for
  operators, never an authorization input, and a wider tag would cost storage
  in every record for no security benefit.

---

## 6. Validation

```bash
# From stellar-lend/
cargo test  -p stellarlend-lending lifecycle
cargo test  -p stellarlend-lending
cargo clippy -p stellarlend-lending --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build -p stellarlend-lending --target wasm32-unknown-unknown --release
```

Test coverage in `src/lifecycle_test.rs`:

| Category | Coverage |
|---|---|
| Success | Each action's happy path; conservation post-check accepts the authorized snapshot |
| Failure | Every `RejectReason` reachable; unique stable codes; correct `FailureClass` mapping; overflow rejected not wrapped; tampered writes caught by `verify_post` |
| Boundary | `0`, `-1`, `1`, exactly `MAX_TRANSITION_AMOUNT`, one over; exact-balance withdraw and full-debt liquidation; ring capacity and eviction order; page-size clamping; offset past the end; empty ring; every latency bucket edge |
| Retry | Duplicate folding; differing amount not folded; same request in a later ledger; escalation exactly at `MAX_RETRY_ATTEMPTS`; streak below the budget does not escalate; recovery accounting and streak reset |
| Permission | Unauthorized caller for all five actions; owner-only actions reject third parties; self-liquidation ban; authorization checked before accounting |
| Throttling | Budget exhaustion; reset on the next ledger; per-actor isolation; commit still succeeds while throttled |

---

## 7. Limitations

* **Pre-existing merge-conflict markers in `src/lib.rs`.** `src/lib.rs` was
  committed with 13 unresolved conflict blocks (`<<<<<<< HEAD` …
  `>>>>>>> 2062294`) spanning the deposit/withdraw/borrow/repay/liquidate
  entrypoints, so the crate does not currently compile on `main`. Resolving
  them is a separate concern with a much larger blast radius than this issue,
  and the conflicting hunks are exactly the lifecycle entrypoints — merging
  them blind would risk silently picking the wrong accounting branch. This
  change is therefore additive and touches no conflicted region: a module, a
  test module, three read-only entrypoints, and two module declarations. The
  validation commands above will not pass until those conflicts are resolved.
* **Entrypoints are not yet wired into the write paths.** `guard()` is the
  intended call shape for `deposit`/`withdraw`/`borrow`/`repay`/`liquidate`,
  but adopting it requires editing exactly the conflicted hunks above. That
  wiring is deliberately left for the follow-up that resolves them.
* **Cross-asset positions are single-leg here.** The snapshot models one
  collateral leg and one debt leg. Multi-asset positions must apply the guard
  per asset pair; the module does not aggregate USD-weighted health, which
  remains `cross_asset.rs`'s responsibility.
* **Liquidation bonus is out of scope.** The guard enforces the debt leg and
  that the base seizure cannot drive collateral negative; the incentive
  multiplier stays with the existing `liquidate` implementation.
* **`actor_tag` collisions** are possible as described in §5.
