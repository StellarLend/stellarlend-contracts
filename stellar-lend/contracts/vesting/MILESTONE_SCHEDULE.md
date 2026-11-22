# Milestone Vesting Schedule Specification & Validation

## Overview

The StellarLend Vesting contract supports milestone-based vesting schedules alongside traditional linear schedules with cliffs. Milestone grants allow discrete tranches of tokens to unlock at specific future (or past) timestamps.

Milestone grants are created by the contract admin via:

```rust
pub fn add_grant_milestone(
    env: Env,
    caller: Address,
    grantee: Address,
    total_amount: i128,
    milestones: Vec<(u64, i128)>,
) -> Result<(), VestingError>
```

Each entry in `milestones` is a tuple `(timestamp, cumulative_amount)` representing the total amount of tokens that will have vested once the ledger timestamp reaches `timestamp`.

---

## Validation Rules & Invariants

To prevent mistyped schedules from locking grantee funds, vesting out of order, or consuming excessive compute/storage gas that could freeze claims, all milestone schedules are strictly validated **up front** in `VestingContract::validate_milestones` before any tokens are transferred or storage is modified.

### 1. Bounded Schedule Length (`1 ≤ len ≤ MAX_MILESTONES`)

- **Rule**: A milestone schedule must contain at least 1 and at most `MAX_MILESTONES` (64) milestones.
- **Constant**: `pub const MAX_MILESTONES: u32 = 64;`
- **Rationale**: Iteration over milestones occurs during `vested_at` calculations for claims, partial claims, revocations, and view calls. Capping the length at 64 bounds CPU instructions, memory footprint, and storage-read costs well within default Soroban transaction limits, while providing sufficient granularity for multi-year vesting schedules (e.g., monthly unlocks over a 5-year grant require 60 milestones).
- **Error Codes**:
  - `len == 0`: rejected with `VestingError::InvalidMilestoneOrder`
  - `len > 64`: rejected with `VestingError::TooManyMilestones`

### 2. Strictly Increasing Timestamps (`t_i > t_{i-1}`)

- **Rule**: Timestamps must be sorted in strictly ascending order:
  $$\forall i > 0, \quad t_i > t_{i-1}$$
- **Duplicate Timestamps**: Disallowed ($t_i = t_{i-1}$ is rejected).
- **Past Timestamps**: Timestamps in the past relative to the current ledger timestamp are permitted. Any milestone whose timestamp is already in the past vests immediately upon schedule creation.
- **Error Code**: Rejected with `VestingError::InvalidMilestoneOrder`.

### 3. Strictly Increasing Cumulative Amounts (`cum_i > cum_{i-1}` and `cum_0 > 0`)

- **Rule**: Cumulative amounts must be strictly positive and strictly ascending:
  $$cum_0 > 0 \quad \text{and} \quad \forall i > 0, \quad cum_i > cum_{i-1}$$
- **Zero or Negative Amounts**: Disallowed ($cum \le 0$ is rejected).
- **Non-Increasing / Decreasing Amounts**: Disallowed ($cum_i \le cum_{i-1}$ is rejected). Every milestone must unlock a strictly positive increment of tokens; a milestone with zero incremental vesting wastes storage and gas.
- **Error Code**: Rejected with `VestingError::InvalidMilestoneOrder`.

### 4. Final Cumulative Amount Equals Escrowed Total (`cum_{last} == total_amount`)

- **Rule**: The cumulative amount of the final milestone must exactly equal the `total_amount` escrowed for the grant:
  $$cum_{N-1} = \text{total\_amount}$$
- **Under-Total Schedules** ($cum_{N-1} < \text{total\_amount}$): Disallowed. Without this check, unallocated tokens would be permanently locked in the contract vault until revoked.
- **Over-Total Schedules** ($cum_{N-1} > \text{total\_amount}$): Disallowed.
- **Error Code**: Rejected with `VestingError::MilestoneTotalMismatch`.

---

## Examples of Rejected Schedules

The table below illustrates common schedule misconfigurations and their corresponding typed rejection errors:

| Schedule & Total Amount | Issue Description | Resulting Error |
| ----------------------- | ----------------- | --------------- |
| `[]`, `total = 100` | Empty schedule (length 0) | `VestingError::InvalidMilestoneOrder` |
| `[(200, 50), (100, 100)]`, `total = 100` | Unsorted timestamps (`200 > 100`) | `VestingError::InvalidMilestoneOrder` |
| `[(100, 50), (100, 100)]`, `total = 100` | Duplicate timestamp (`100 == 100`) | `VestingError::InvalidMilestoneOrder` |
| `[(100, 70), (200, 50)]`, `total = 50` | Decreasing cumulative amount (`70 -> 50`) | `VestingError::InvalidMilestoneOrder` |
| `[(100, 50), (200, 50)]`, `total = 50` | Non-increasing cumulative amount (`50 -> 50`) | `VestingError::InvalidMilestoneOrder` |
| `[(100, 0), (200, 100)]`, `total = 100` | Zero cumulative amount (`cum = 0`) | `VestingError::InvalidMilestoneOrder` |
| `[(100, -20), (200, 100)]`, `total = 100` | Negative cumulative amount (`cum = -20`) | `VestingError::InvalidMilestoneOrder` |
| `65` milestones, `total = 650` | Schedule length exceeds `MAX_MILESTONES` (64) | `VestingError::TooManyMilestones` |
| `[(100, 40), (200, 80)]`, `total = 100` | Final amount (80) under-totals escrow (100) | `VestingError::MilestoneTotalMismatch` |
| `[(100, 50), (200, 150)]`, `total = 100` | Final amount (150) over-totals escrow (100) | `VestingError::MilestoneTotalMismatch` |

---

## Atomicity and Call Ordering

Validation is executed before token transfer and storage mutations:

1. Caller authorization is verified (`require_admin`).
2. Principal positivity is verified (`total_amount > 0`).
3. Schedule invariants are verified (`validate_milestones`).
4. **Token transfer is performed** only after all checks pass:
   ```rust
   token::Client::new(&env, &token_address).transfer(
       &caller,
       env.current_contract_address(),
       &total_amount,
   );
   ```
5. Grant is stored and `TotalLocked` is incremented.

If validation fails on any invariant, the transaction aborts with the specific `VestingError`. **No tokens are transferred, no storage is written, and no events are emitted.**

---

## Vesting Calculation & Monotonicity

Because milestone entries are guaranteed to be sorted by timestamp and have strictly increasing cumulative amounts, `Grant::vested_at(now)` computes:

```rust
match &self.schedule {
    VestingSchedule::Milestone(milestones) => {
        let mut vested: i128 = 0;
        for i in 0..milestones.len() {
            let (ts, cum) = milestones.get(i).unwrap();
            if now >= ts {
                vested = cum;
            } else {
                break;
            }
        }
        vested.min(self.total_amount)
    }
    // ...
}
```

### Guarantees
- **Exact Vesting**: At $t = t_i$, exactly $cum_i$ tokens are vested.
- **Full Vesting**: At or after the final milestone $t \ge t_{N-1}$, exactly `total_amount` is vested.
- **Monotonicity**: For any $t_1 \le t_2$, $\text{vested\_at}(t_1) \le \text{vested\_at}(t_2)$.
- **No Overflow**: Milestone evaluation uses comparisons rather than arithmetic additions, ensuring safe execution even when `total_amount` approaches `i128::MAX`.

---

## Interaction with Revoke and Acceleration

1. **Revocation (`revoke`)**:
   - Computes `vested = grant.vested_at(effective_now)`.
   - Claws back `total_amount - vested` to the treasury.
   - Clamps `grant.total_amount = vested` and sets `grant.revoked = true`.
   - Subsequent calls to `grant.vested_at(t)` remain capped at the clamped `grant.total_amount`, ensuring unvested tokens are never unlocked after revocation.

2. **Acceleration (`accelerate_grant`)**:
   - Converts the milestone grant into a linear grant (`schedule = VestingSchedule::Linear`, `start_ts = 0`, `cliff_secs = 0`, `duration_secs = 1`).
   - Immediately makes 100% of unvested tokens claimable by the grantee.
