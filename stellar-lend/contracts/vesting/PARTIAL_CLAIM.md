# Partial Claim Feature

## Rationale

The vesting contract's original `claim` entrypoint always claims the full claimable amount across all grants for a grantee. The `claim_partial` entrypoint adds flexibility by allowing the grantee to claim any amount up to the claimable balance, enabling:

1. **Gradual liquidity**: Grantees can claim smaller amounts as needed, reducing market impact
2. **Better cash flow management**: Users can claim amounts matching their immediate expenses
3. **Integration flexibility**: Downstream protocols can pull exact amounts for specific operations

## Worked Example

Consider a grantee "alice" with a single grant of 10,000 tokens:

- `start_seconds`: 1,000
- `cliff_seconds`: 200
- `duration_seconds`: 800
- Total: 10,000 tokens

### Time progression:

| Time | Vested amount | Previously claimed | Claimable |
|------|---------------|-------------------|-----------|
| 1,000 | 0 (cliff not passed) | 0 | 0 |
| 1,199 | 0 (cliff not passed) | 0 | 0 |
| 1,200 | 250 (just after cliff) | 0 | 250 |
| 1,500 | 625 | 0 | 625 |
| 2,000 | 2,500 | 500 | 2,000 |

### Partial claim flow at t=1,500:
1. Call `claim_partial("alice", 300, 1500)`
2. Contract syncs: `vested_at(1500) = 10000 * 300 / 800 = 3750`... wait, let me recalculate.

Actually, the cliff is 200 seconds after start (1000 + 200 = 1200). At t=1500, elapsed = 500 seconds.
- `vested = 10000 * 500 / 800 = 6250`
- Claimable = 6250 - 0 = 6250

If alice calls `claim_partial("alice", 300, 1500)`:
1. Sync runs, setting `released = 6250`
2. Check: `amount (300) <= claimable (6250)` ✓
3. Update: `claimed += 300`, `balance_of("alice") += 300`

If alice then calls `claim_partial("alice", 500, 1500)`:
1. Sync runs, but no change (already synced)
2. Check: `amount (500) <= claimable (5950)` ✓
3. Update: `claimed += 500`, `balance_of("alice") += 500`

## Edge Cases

### Zero amount
- `claim_partial("alice", 0, now)` returns `InvalidAmount` error
- No state is mutated

### Amount exceeds claimable
- `claim_partial("alice", amount, now)` where `amount > claimable()` returns `OverClaim` error
- `sync_grants` still runs (vesting math updates `released`), but `claimed` is not updated

### Contract paused
- Both `claim` and `claim_partial` check pause gate before any state mutation
- Returns `ContractPaused` error
- For `claim_partial`, sync does NOT run (unlike OverClaim case)

### Multiple grants
- When a grantee has multiple grants, the partial amount is drawn from the first grant(s) first
- Unconsumed claimable remains available after the partial claim

### Negative amounts
- `u128` cannot be negative, so this is implicitly handled

### Overflow protection
- All `claimed` accumulator updates use `checked_add`
- Saturating arithmetic is used throughout to prevent underflow