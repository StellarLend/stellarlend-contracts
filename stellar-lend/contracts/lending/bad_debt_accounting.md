# Bad Debt Accounting Specification

The protocol guards against system insolvency using an on-chain bad-debt accumulator tracker.

## Shortfall Accumulator Mechanism
When a liquidation event triggers, the liquidator is entitled to an incentivized collateral amount (`seized_collateral`). If the user's positions drop below safe limits such that `seized_collateral > available_collateral`, the system clamps the payout and tracks the shortfall directly.

$$\text{Shortfall} = \text{Seized Collateral} - \text{Available Collateral}$$

This value is stored via `DataKey::BadDebt` using state instance storage.

### Worked Example
- **Incentivized Seizure Target:** 150 Collateral Tokens
- **Available Borrower Collateral:** 100 Collateral Tokens
- **Shortfall Recorded:** 50 Tokens added monotonically to the BadDebt ledger.

### Invariants
- **Monotonic Increase:** Bad debt values can never decrease except via explicit, future administrative `write_off_bad_debt` entrypoints (currently left as a TODO).
- **Checked Arithmetic:** All computations run inside safe `checked_sub` and `checked_add` parameters to rule out integer overflow vulnerabilities.
- **Bad Debt ($D_{bad}$)**: The unrecovered portion of a borrow position after all available collateral has been liquidated.
- **Insurance Fund ($F_{ins}$)**: A per-asset pool of tokens (maintained via accounting) used to offset bad debt.
- **Underwater Position**: A position where `Collateral Value * Liquidation Threshold < Debt Value`.
- **Insolvent Position**: A position where `Collateral Value < Debt Value`.

## 3. Storage Structures

Three primary keys are added to the `Instance` storage to track global accounting:

| Key | Type | Description |
|-----|------|-------------|
| `TotalBadDebt(Address)` | `i128` | Cumulative unrecovered debt for a specific asset. |
| `InsuranceFundBalance(Address)` | `i128` | Current balance of the insurance fund for a specific asset. |
| `SocializedLoss(Address)` | `i128` | (Optional) Track debt that has been written off. |

## 4. Accounting Invariants

The protocol maintains the following global invariants (where $D_{total}$ is total debt and $C_{total}$ is total collateral):

1. **Global Solvency**: $\sum C_{val} \ge \sum D_{val} - \sum D_{bad}$.
2. **Insurance Fund Integrity**: $F_{ins} \ge 0$.
3. **Bad Debt Tracking**: $D_{bad}$ only increases during insolvent liquidations and decreases during an `Offset` event.

## 5. Liquidation Flow with Bad Debt

When `liquidate_position` is called:

1. **Calculate Recoverable Debt**: Determine how much of the user's debt can be covered by their remaining collateral (including the liquidator incentive).
2. **Handle Insolvent Case**:
   - If `Repay Amount > Collateral Value`:
     - $Shortfall = Repay Amount - Collateral Value$.
     - Increase `TotalBadDebt(Asset)` by $Shortfall$.
     - If `InsuranceFundBalance(Asset) > 0`:
       - $Offset = \min(Shortfall, InsuranceFundBalance)$.
       - Decrease `InsuranceFundBalance` and `TotalBadDebt` by $Offset$.
3. **Emit Events**: `BadDebtRecorded` and `InsuranceFundOffset`.

## 6. Access Control & Authorization

- **Crediting Fund**: Admin or specific protocol-authorized addresses (e.g., from flash loan fees) can credit the `InsuranceFundBalance`.
- **Offsetting Debt**: Automated during liquidation or manually triggered by the `Admin` or `Guardian` during emergency recovery.
- **Checked Arithmetic**: All updates to $D_{bad}$ and $F_{ins}$ MUST use `checked_add` and `checked_sub` to prevent overflow-driven insolvency.

## 7. Reentrancy & Security

- **Checks-Effects-Interactions**: Accounting updates to $D_{bad}$ and $F_{ins}$ occur BEFORE any external token transfers (if any) are simulated or executed.
- **Authorization**: `require_auth()` is enforced on all admin functions modifying the insurance fund or writing off debt.




# Bad-Debt Accounting

_StellarLend Protocol — `stellar-lend/contracts/lending/bad_debt_accounting.md`_

---

## 1. Overview

Bad debt arises when a borrower's position becomes insolvent before a liquidator
can act — typically because collateral value falls faster than oracle updates or
because no external liquidator was incentivised to close the position in time.

The protocol handles bad debt through a two-tier absorption model:

```
shortfall
   │
   ├─── up to reserves ──▶  Protocol Reserves  (first loss)
   │
   └─── remainder       ──▶  Bad-Debt Ledger   (socialised, recovered over time)
```

No user borrow balance is ever left in a partially-written-off state.
Either the full debt is recovered by collateral liquidation, or the entire
residual is written off and the borrower's balance is set to zero.

---

## 2. Terminology

| Term | Definition |
|------|-----------|
| **Shortfall** | `debt_usd - collateral_usd` after seizing all available collateral |
| **Write-off** | Removing the shortfall from the borrower's on-books balance |
| **Reserve cover** | Reserves consumed before any loss is socialised |
| **Bad debt** | Cumulative written-off losses not yet recovered from reserves |
| **Recovery** | Bad debt reduced when new reserves flow in (interest, fees, governance top-ups) |

---

## 3. Accounting Invariants

The following invariants must hold after every state-mutating call.
A violation causes the transaction to abort (`LendingError::BadDebtNegative`
or `LendingError::ReservesNegative`).

| ID | Invariant | Checked by |
|----|-----------|-----------|
| I-1 | `bad_debt >= 0` | `assert_market_invariants` |
| I-2 | `reserves >= 0` | `assert_market_invariants` |
| I-3 | `total_borrows >= 0` | `assert_market_invariants` |
| I-4 | `total_deposits >= 0` | `assert_market_invariants` |
| I-5 | `total_deposits - total_borrows + reserves - bad_debt >= 0` _(nominal solvency)_ | logged, not aborted |
| I-6 | After write-off: `user.borrowed == 0` | `record_bad_debt` |

**Note on I-5:** When `bad_debt > reserves`, the protocol is nominally insolvent —
depositors bear an economic loss equal to `bad_debt - reserves`.  This is an
expected economic outcome (not a code bug) and the protocol continues operating
in degraded mode.  Governance recovers solvency by topping up reserves.

---

## 4. Write-Off Flow

```
liquidate()  ──or──  emergency_liquidate()
        │
        │  collateral fully seized, residual > 0?
        │
        ▼
record_bad_debt(user, asset, residual, collateral_seized)
        │
        ├─ 1. Zero user.borrow                      [I-6]
        ├─ 2. Decrement market.total_borrows
        ├─ 3. reserve_cover = min(residual, reserves)
        ├─ 4. market.reserves -= reserve_cover      [I-2]
        ├─ 5. written_off = residual - reserve_cover
        ├─ 6. market.bad_debt += written_off        [I-1]
        ├─ 7. Store per-user audit record
        ├─ 8. assert_market_invariants()
        └─ 9. Persist market state
```

---

## 5. Recovery Flow

```
reserves increase (interest / fees / governance top-up)
        │
        ▼
attempt_bad_debt_recovery(asset, new_reserves)
        │
        ├─ recovery = min(new_reserves, market.bad_debt)
        ├─ market.bad_debt -= recovery              [I-1]
        ├─ market.reserves += (new_reserves - recovery)
        └─ assert_market_invariants()
```

Recovery is monotonically non-increasing for `bad_debt`: each call either
reduces it or leaves it unchanged.  It can never make `bad_debt` negative.

---

## 6. Emergency Shutdown

When governance triggers an emergency shutdown:

1. Global shutdown flag is set — new borrows and deposits are rejected.
2. Every specified market is **frozen** (same effect for deposits/borrows).
3. **Liquidations remain open** so bad debt can still be cleared.
4. `emergency_liquidate()` bypasses the close factor — the full position is
   liquidated in a single call.

### Shutdown accounting example

| Step | Action | Result |
|------|--------|--------|
| 1 | Borrower has 1 ETH @ $500 collateral, 1000 USDC debt | shortfall = $500 |
| 2 | `emergency_liquidate()` seizes 1 ETH → $500 value | collateral_seized = 1 ETH |
| 3 | `record_bad_debt(residual=$500)` | reserve_cover = min($500, reserves) |
| 4 | reserves = 0 → written_off = $500 | market.bad_debt += $500 |
| 5 | User borrow zeroed | [I-6] satisfied |
| 6 | Governance tops up $500 reserves | bad_debt → 0 |

---

## 7. Partial Liquidation and Residual Debt

The **close factor** (50% by default) allows a liquidator to repay at most
half the outstanding debt in a single call.  This means:

- A partially liquidated position remains open.
- The remaining debt can be liquidated in subsequent calls once the position
  is still undercollateralised.
- Bad debt only accrues when:
  - All available collateral is seized _and_
  - The collateral value was still insufficient to cover the repaid amount
    (i.e., the collateral was exhausted mid-liquidation).

### Partial liquidation example

```
Borrower:  2 ETH @ $800 = $1600 collateral, $2000 USDC debt
CF = 75% → max_borrow = $1200 → position UNHEALTHY

Liquidation call 1:  repay 1000 USDC (50% close factor)
  seized = 1000 * $1 * 1.08 / $800 = 1.35 ETH
  remaining borrow = 1000 USDC
  remaining collateral = 0.65 ETH

Liquidation call 2:  repay 1000 USDC
  seized = min(1.35 ETH, 0.65 ETH) = 0.65 ETH ($520 value)
  residual = $1000 − $520 = $480
  record_bad_debt(residual=$480)
```

---

## 8. Oracle Price Constraints

- All prices are in **micro-USD** (1 USD = `PRICE_PRECISION = 1_000_000`).
- Hard floor: `MIN_PRICE = 1` — prices below this are rejected with
  `LendingError::OraclePriceTooLow`, preventing infinite-leverage attacks.
- In production, staleness > `MAX_AGE_LEDGERS` ledgers is also rejected.

---

## 9. Security Notes

### Solvency assumptions
- The protocol is only solvent if `reserves >= bad_debt`.  When this fails,
  depositors bear a haircut proportional to their share of `total_deposits`.
- Governance is responsible for maintaining a reserve buffer sufficient to
  absorb expected bad debt in tail-risk scenarios.

### Re-entrancy
- All state writes are committed atomically in Soroban's transaction model.
  There is no external call between reading and writing state in
  `record_bad_debt`, so re-entrancy is structurally impossible.

### Oracle manipulation
- A manipulated oracle price could make a healthy position appear unhealthy
  or vice versa.  The MIN_PRICE floor prevents the most damaging case
  (collateral price → 0 opening infinite borrows).
- Production deployments should use a TWAP or circuit-breaker oracle.

### Integer arithmetic
- All arithmetic uses `checked_mul` / `saturating_sub` to prevent overflow
  and underflow respectively.
- Prices and amounts are kept in 6-decimal-place integer units throughout.

### Close factor protection
- Without a close factor, a liquidator could seize all collateral from a
  position that is only marginally undercollateralised, leaving the borrower
  with no recourse.  The 50% close factor limits each liquidation's impact.

---

## 10. Accounting Examples

### Example A — Full liquidation, no bad debt

```
State:   1 ETH @ $1200, borrow 500 USDC  (unhealthy: max = $900)
Repay:   500 USDC
Seized:  500 × $1 × 1.05 / $1200 ≈ 0.4375 ETH ($525 value)

bad_debt  = 0     ✓
reserves  = 0     (unchanged)
```

### Example B — Collateral collapse, full write-off

```
State:   1 ETH @ $0.001, borrow 1000 USDC
Seized:  1 ETH → $0.001 value
Residual: $999.999 ≈ $1000

Reserves = 200 USDC:
  reserve_cover = 200 USDC
  written_off   = 800 USDC
  bad_debt      = 800 USDC
  reserves      = 0

Later: governance tops up 800 USDC:
  bad_debt → 0
  reserves → 0
```

### Example C — Sequential recovery

```
bad_debt = 900 USDC  (3 users × 300 USDC each)

Top-up 1: add_reserves(300)  → bad_debt = 600
Top-up 2: add_reserves(300)  → bad_debt = 300
Top-up 3: add_reserves(300)  → bad_debt = 0   ✓
```

---

## 11. Governed Write-Off (`write_off_bad_debt`)

_Entrypoint added in feature/bad-debt-write-off._

### 11.1 Purpose

`record_bad_debt` causes `bad_debt` to grow monotonically. Without a
resolution path, the protocol remains nominally insolvent indefinitely.
`write_off_bad_debt(amount)` provides the governed, auditable mechanism to
clear recorded bad debt against available backstops.

**Access control**: admin only (`require_auth` via `assert_admin`).  
**Atomicity**: all state mutations precede the event — no external call is
made between reads and writes.

---

### 11.2 Backstop Precedence

| Priority | Source | Storage key | Notes |
|----------|--------|-------------|-------|
| 1 (first) | Insurance Fund | `InsuranceFund` (instance) | Credited by governance via `credit_insurance_fund` |
| 2 | Protocol Reserve | `TotalDeposits` (persistent) | Deposit-pool surplus absorbed next |
| 3 (last) | Depositor Socialisation | `TotalDeposits` (persistent) | Index haircut — irreversible economic loss |

The insurance fund is consumed first because it was explicitly set aside for
exactly this purpose.  Protocol reserves (the depositor pool) act as the
second absorber.  Only when _both_ are exhausted is the residual socialised
across depositors via a `TotalDeposits` reduction (equivalent to an index
haircut proportional to each depositor's share).

> **Safety constraint**: `TotalDeposits` can never become negative.
> If the socialised amount would reduce `TotalDeposits` below zero, the
> transaction is rejected with `LendingError::Overflow`.  Governance must
> ensure `insurance + total_deposits ≥ amount` before calling the entrypoint.

---

### 11.3 State-Machine Diagram

```
write_off_bad_debt(amount)
        │
        ├─ GUARD: amount > 0                  → InvalidAmount on fail
        ├─ GUARD: bad_debt > 0                → NoBadDebt on fail
        ├─ GUARD: amount ≤ bad_debt           → WriteOffExceedsBadDebt on fail
        │
        ├─ 1. insurance_used = min(amount, insurance_fund)
        │     insurance_fund  -= insurance_used
        │     remaining        = amount - insurance_used
        │
        ├─ 2. reserve_used    = min(remaining, total_deposits)
        │     total_deposits  -= reserve_used
        │     remaining       -= reserve_used
        │
        ├─ 3. socialized       = remaining
        │     total_deposits  -= socialized        ← checked_sub (Overflow if < 0)
        │
        ├─ 4. bad_debt        -= amount
        │
        └─ 5. Emit BadDebtWrittenOffEvent {
                  amount, insurance_used,
                  reserve_used, socialized
              }
```

**Invariant check** (post-call):

| Invariant | Guaranteed |
|-----------|-----------|
| `bad_debt >= 0` | ✓ — `amount ≤ bad_debt` guard |
| `insurance_fund >= 0` | ✓ — `insurance_used = min(amount, insurance_fund)` |
| `total_deposits >= 0` | ✓ — `checked_sub` aborts if negative |
| `insurance_used + reserve_used + socialized == amount` | ✓ — algebraic identity |

---

### 11.4 Worked Example — Mixed Coverage

```
Initial state:
  bad_debt       = 1 000 USDC
  insurance_fund =   300 USDC   (credited by governance)
  total_deposits = 1 500 USDC   (aggregate depositor value)

Call: write_off_bad_debt(1_000)

Step 1 — Insurance fund:
  insurance_used = min(1 000, 300) = 300
  insurance_fund → 300 - 300     =   0
  remaining      = 1 000 - 300   = 700

Step 2 — Protocol reserve (TotalDeposits):
  reserve_used     = min(700, 1 500) = 700
  total_deposits   → 1 500 - 700    = 800
  remaining        = 700 - 700      =   0

Step 3 — Depositor socialisation:
  socialized       = 0   (nothing left to absorb)
  total_deposits   → 800 - 0 = 800   (unchanged)

Step 4 — Clear bad debt:
  bad_debt         → 1 000 - 1 000 = 0

Event emitted:
  BadDebtWrittenOffEvent {
      amount:         1 000,
      insurance_used:   300,
      reserve_used:     700,
      socialized:         0,
  }

Post state:
  bad_debt       =     0 USDC  ✓
  insurance_fund =     0 USDC
  total_deposits =   800 USDC  (depositors bear no haircut in this scenario)
```

---

### 11.5 Worked Example — Depositor Haircut Required

```
Initial state:
  bad_debt       = 1 000 USDC
  insurance_fund =   200 USDC
  total_deposits =   600 USDC

Call: write_off_bad_debt(600)  ← governance writes off only 600 (within pool capacity)

Step 1 — Insurance fund:
  insurance_used = min(600, 200) = 200
  insurance_fund → 0
  remaining      = 600 - 200   = 400

Step 2 — Protocol reserve:
  reserve_used   = min(400, 600) = 400
  total_deposits → 600 - 400    = 200
  remaining      = 400 - 400    =   0

Step 3 — Socialisation:
  socialized = 0

Step 4 — Clear bad debt (partial):
  bad_debt → 1 000 - 600 = 400   ← 400 USDC bad debt remains

Event:
  BadDebtWrittenOffEvent {
      amount: 600, insurance_used: 200, reserve_used: 400, socialized: 0
  }

Governance must call write_off_bad_debt again after topping up backstops
to clear the remaining 400 USDC of bad debt.
```

---

### 11.6 Security Notes

#### Over-write prevention
`amount > bad_debt` is checked before any state mutation.
Attempting to clear more bad debt than exists is rejected with
`WriteOffExceedsBadDebt` (error code 3002).

#### Checked arithmetic throughout
Every subtraction uses `checked_sub(...).ok_or(LendingError::Overflow)`.
A transaction is atomically reverted on any arithmetic failure — no
partial state is written.

#### Irreversibility of socialisation
A depositor haircut (non-zero `socialized` field) permanently reduces
`TotalDeposits`. There is no undo path.  Governance should exhaust all
insurance and reserve options before allowing any haircut to occur.

#### Re-entrancy safety
`write_off_bad_debt` makes no external calls.  All reads precede all
writes.  The function is structurally re-entrancy-safe under Soroban's
single-threaded execution model.

#### Auth requirement
`assert_admin(&env)` calls `admin.require_auth()` at the top of the
function — before any state is read or modified.  A non-admin invocation
will trap immediately.
