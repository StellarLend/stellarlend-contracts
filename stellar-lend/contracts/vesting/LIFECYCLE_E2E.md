# Vesting Lifecycle E2E Conservation Tests

**Issue:** #1228  
**File under test:** `src/lib.rs` → `VestingContract` (`add_grant`, `claim`, `claim_partial`, `revoke`)

---

## Rationale

The vesting contract has per-entrypoint unit tests but no end-to-end test that
asserts **global balance conservation** across a full lifecycle. If `add_grant`,
`claim`, and `revoke` each work individually but interact incorrectly, tokens
could be silently created or destroyed.

This test suite drives the contract through the complete `add_grant → partial
claim → revoke` lifecycle and asserts at every step that:

```
claimed + clawback_to_treasury + locked == original_principal
```

---

## Worked Example

**Setup:** 1 000 tokens, start = t=0, duration = 1 000 s, cliff = 200 s.

| Time | Event | Alice balance | Treasury | total_locked | Conservation |
|------|-------|--------------|----------|--------------|-------------|
| t=0 | `add_grant` | 0 | 0 | 1 000 | 0+0+1000 = 1000 ✓ |
| t=100 | `claim` (before cliff) | 0 | 0 | 1 000 | 0+0+1000 = 1000 ✓ |
| t=500 | `claim_partial(300)` | 300 | 0 | 700 | 300+0+700 = 1000 ✓ |
| t=500 | `revoke` | 300 | 500 | 0 | 300+500+0 = 800 ✗* |
| t=500 | alice claims remaining 200 | 500 | 500 | 0 | 500+500+0 = 1000 ✓ |

\* Between revoke and the final claim, the 200 vested-but-unclaimed tokens sit
in `balance_of("contract")`. The invariant holds when contract balance is
included: `alice + contract + treasury + locked = 1000` throughout.

---

## Edge Cases

### Revoke before the cliff (t < cliff)

No tokens have vested. The entire principal is still locked (unvested).
`revoke` claws back 100% of the principal to the treasury. Alice receives
nothing and has no remaining claimable balance.

```
clawback == principal
alice == 0
total_locked == 0
```

### Revoke after partial vesting (cliff ≤ t < duration)

Some tokens have vested, the rest are still locked.

```
clawback == total - vested_at(t)
alice can still claim vested_at(t) - already_claimed
total_locked == 0  (after revoke)
```

### claimed + clawback + locked invariant

`total_locked` tracks only **unvested** tokens. After a claim, `total_locked`
decrements by the newly-released amount. After a revoke, `total_locked` drops
to zero (all unvested tokens transferred to treasury).

The full conservation identity is therefore:

```
balance_of(grantee) + balance_of(treasury) + total_locked == principal
```

This holds at every point: before cliff, mid-vest, post-claim, and post-revoke.

### balance_of consistency

`balance_of("contract")` equals `principal - balance_of(grantee) -
balance_of("treasury")` at all times. Each claim reduces the contract balance
by exactly the claimed amount and increases the grantee's balance by the same.

### total_locked consistency

`total_locked` is decremented in `sync_grants` as time advances (newly vested
tokens are released from lock), and decremented in `revoke` when unvested
tokens are clawed back. It is never negative.

---

## Test Matrix (`lifecycle_e2e_test.rs`)

| Test | Requirement |
|------|-------------|
| `conservation_after_add_grant` | Baseline invariant |
| `conservation_before_cliff` | Pre-cliff zero-claim |
| `conservation_after_partial_claim_mid_vest` | Partial claim conservation |
| `full_lifecycle_partial_claim_then_revoke` | Full add→claim→revoke lifecycle |
| `revoke_before_cliff_claws_back_entire_principal` | Revoke before cliff |
| `revoke_after_partial_vest_no_prior_claim` | Revoke after partial vest |
| `total_locked_decrements_with_vesting_progress` | total_locked consistency |
| `balance_of_consistency_across_claims` | balance_of consistency |
| `double_revoke_returns_already_revoked` | Error path – double revoke |
| `revoke_by_non_admin_returns_unauthorized` | Error path – auth check |
| `timeline_across_cliff_and_mid_vest` | Realistic timeline, all steps |
