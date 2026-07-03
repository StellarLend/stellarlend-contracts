# Vesting Revoke: Auth + Treasury Conservation Hardening

> **Status:** Design doc. The `vesting` crate does not currently compile on
> `main` (pre-existing, unrelated breakage in `lib.rs`). This document specifies
> the required checks and the test matrix now; the code change to `revoke` and
> `revoke_conservation_test.rs` will follow once the crate builds.

## Current behavior (`Vesting::revoke`)

`revoke(caller, grantee, now)` today:

1. Rejects `caller != admin` with `Unauthorized`.
2. Checks the pause gate (`check_not_paused`) **after** the auth check, so an
   unauthorized caller never learns the pause state.
3. `sync_grants(grantee, now)` so `released` reflects the vested amount at `now`.
4. For each non-revoked grant: `unvested = grant.locked()`, accumulates
   `transfer`, decrements `total_locked`, sets `grant.total = grant.released`,
   marks `grant.revoked = true`.
5. Moves `min(contract_balance, transfer)` from `"contract"` to the treasury.

## Threat / gap

Auth and treasury routing are already tested, but the claw-back has **no explicit
conservation post-condition**. Two subtle failure modes are unguarded:

- **Stranded grantee funds** — an off-by-one between vested and claimed could
  reduce the grantee's still-claimable balance below what they already earned.
- **Treasury over-credit** — the treasury could be credited more than the true
  unvested remainder, effectively minting principal.

Additionally, step 5 silently clamps `transfer` to the contract balance
(`actual_transfer = min(cbal, transfer)`). If `cbal < transfer`, the treasury is
**under-credited** and the discrepancy is invisible — a conservation violation
that should at least be surfaced.

## Required checks (to implement in `revoke`)

Let, for the revoked set:

- `principal` = Σ `grant.total` (pre-revoke, post-sync)
- `already_claimed` = Σ `grant.claimed`
- `grantee_still_claimable` = Σ `grant.claimable()` after revoke (vested-unclaimed)
- `treasury_credit` = amount actually moved to the treasury

1. **Re-assert admin auth** at the top of `revoke` (keep the existing
   `caller != admin -> Unauthorized`, before the pause check).
2. **Positive treasury balance / liquidity check** — before crediting, require
   the contract actually holds the unvested remainder:
   `contract_balance >= transfer`. If not, return a distinct error
   (`InsufficientTreasuryBalance`) instead of silently under-crediting. The
   treasury address itself must be set and non-empty.
3. **Checked arithmetic** — replace `transfer += unvested` and the balance
   mutations with `checked_add` / `checked_sub`; reject on any underflow rather
   than `saturating_*` (saturation hides conservation bugs).
4. **Conservation post-condition** — after the split, assert:

   ```
   grantee_still_claimable + treasury_credit == principal - already_claimed
   ```

   i.e. no unit of principal is created or destroyed across the claw-back.
5. **Event** — emit `GrantRevoked { grantee, clawed_back, retained }` where
   `clawed_back == treasury_credit` and `retained == grantee_still_claimable`,
   for indexers.
6. **Preserve** existing pause-state gating and revoke-split tests.

## Conservation equation (worked example)

Grant: `total = 1000`, cliff passed, `released = 400` (vested), `claimed = 100`.

- `unvested = locked() = total - released = 600` -> treasury.
- `grantee_still_claimable = released - claimed = 300`.
- `principal - already_claimed = 1000 - 100 = 900`.
- Check: `300 (retained) + 600 (clawed_back) = 900` ✓.

## Test cases (`src/revoke_conservation_test.rs`)

NatSpec-style `///` doc comments on each helper.

1. **Auth** — non-admin caller -> `Unauthorized`; no balances mutated.
2. **Pause ordering** — paused + non-admin -> still `Unauthorized` (pause not
   leaked); paused + admin -> `ContractPaused`, no mutation.
3. **Conservation, partially vested** — the worked example above; assert the
   equation holds to the unit and the `GrantRevoked` event fields match.
4. **Fully vested** — `released == total`; `clawed_back == 0`,
   `retained == total - claimed`, equation holds.
5. **Nothing vested** — before cliff; `clawed_back == total`, `retained == 0`.
6. **Insufficient treasury liquidity** — contract balance `< transfer` ->
   `InsufficientTreasuryBalance`, no partial/silent under-credit.
7. **Already revoked** — second revoke -> `AlreadyRevoked`, balances unchanged.
8. **Underflow guard** — crafted `claimed > released` state (if reachable) is
   rejected by checked arithmetic rather than wrapping.
