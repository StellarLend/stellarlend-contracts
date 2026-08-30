# Grant Acceleration

## Rationale

Vesting schedules are designed for the common case: an employee or contributor
earns tokens linearly over time. But some corporate events — acquisitions,
change-of-control clauses, involuntary terminations with acceleration provisions,
or emergency recovery scenarios — require that a grant become **immediately and
fully claimable** regardless of how much time has elapsed.

### Why not a time-based override?

A time-based override (e.g., "set `start_seconds` far in the past") could be
gamed by anyone who can call the function, and it complicates `vested_at` math for
future queries. More importantly, it conflates two distinct concepts: *when* a
grant vests vs. *who decides* it vests early.

### Why an admin-gated function?

- **Single accountability point.** Only the configured admin can trigger
  acceleration, ensuring it cannot be self-served by grantees.
- **Audit trail.** The `GrantAccelerated` on-ledger event provides an immutable
  record of every acceleration, including the grantee, the amount newly released,
  and the timestamp.
- **Orthogonality.** Acceleration is additive: it does not change `claimed` (the
  tokens already withdrawn), it does not revoke anything, and it does not interact
  with `pause` beyond the standard settlement gate.

---

## How It Works

`accelerate_grant(caller, grantee, now)` does the following for every active
(non-revoked) grant belonging to `grantee`:

1. Sets `released = total` — the full principal is immediately claimable.
2. Rewrites the schedule fields so that `vested_at(t) == total` for all `t >= 1`:
   ```
   start_seconds    = 0
   cliff_seconds    = 0
   duration_seconds = 1
   ```
3. Leaves `claimed` **unchanged** — tokens already withdrawn stay accounted for.
4. Decrements `total_locked` by the newly-unlocked delta `(total - released_before)`.
5. Emits a `GrantAccelerated` event.
6. Extends the grantee's storage TTL so the record does not expire before claim.

After the call, `claimable() == total - claimed` for every active grant.

---

## Worked Example

**Setup**

| Field             | Value      |
|-------------------|------------|
| `total`           | 12 000     |
| `start_seconds`   | 1 000      |
| `duration_seconds`| 2 000      |
| `cliff_seconds`   | 200        |
| `claimed`         | 3 000      |
| Current time      | 1 500      |

**State before `accelerate_grant`**

```
vested_at(1500) = 12000 * (1500 - 1000) / 2000 = 3000
released        = 3000   (synced by a prior claim)
claimed         = 3000
claimable()     = released - claimed = 3000 - 3000 = 0
total_locked    = 9000   (12000 - 3000 released)
```

**Call: `accelerate_grant("admin", "alice", 1500)`**

**State after `accelerate_grant`**

```
released        = 12000  (= total)
claimed         = 3000   (unchanged)
claimable()     = 12000 - 3000 = 9000
total_locked    = 0      (decreased by delta = 12000 - 3000 = 9000)
```

**Event emitted**

```json
GrantAccelerated {
  grantee:   "alice",
  amount:    9000,
  timestamp: 1500
}
```

**Claim after acceleration**

```
claim("alice", 1500)  →  transfers 9000 tokens to alice
claimable()           →  0
contract balance      →  0
```

---

## Error Ordering

Calls fail in this order, so early failures do not leak information:

1. `Unauthorized` — caller is not the admin.
2. `ContractPaused` — the contract is paused.
3. `NoSuchGrant` — the grantee has no recorded grants.
4. `Overflow` — checked arithmetic failed (unreachable under normal invariants).

---

## Edge Cases

### (a) Accelerating a partially-claimed grant

`claimed` is never modified by `accelerate_grant`. If a grantee has already
withdrawn some tokens, those are still reflected in `claimed`, and only
`total - claimed` remains claimable after acceleration.

*Example:* `total=1000, claimed=400` → after acceleration `claimable()=600`.

### (b) Accelerating an already fully-vested grant (idempotency)

If every active grant already has `released == total`, `accelerate_grant` returns
`Ok(())` without modifying any state, emitting any event, or extending the TTL.
This means retry logic (e.g., after a network timeout) is safe to apply.

### (c) Calling while the contract is paused

The standard pause gate applies. `accelerate_grant` is a settlement operation and
is blocked while the contract is paused, returning `VestingError::ContractPaused`.
Importantly, the auth check happens **before** the pause check, so an unauthorised
caller always gets `Unauthorized`, never learning whether the contract is paused.

### (d) Interaction with `revoke` after acceleration

After `accelerate_grant`, every active grant has `released == total` and
`locked() == 0`. A subsequent `revoke` call will:

- Iterate non-revoked grants.
- For each, compute `locked = total - released = 0`.
- Transfer `0` tokens to the treasury.
- Set `revoked = true`.

The grantee retains the unclaimed balance (`total - claimed`) and can still claim
it after the revoke, because `claim` does not require `revoked == false` — it only
skips the `claimable()` accumulation for revoked grants.

> **Note:** Once a grant is revoked, it cannot be un-revoked. Accelerating before
> revoking preserves the grantee's full `total - claimed` balance; revoking first
> claws back only the unvested portion at that moment.
