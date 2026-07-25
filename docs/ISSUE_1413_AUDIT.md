# Issue #1413 — audit + caveat (companion to the crate-level rustdoc note)

<!-- Bound to the docs-only closure of #1413. Delete this file when
     `flash_swap_a_for_b` and `repay_flash_swap` are actually
     implemented. -->

<!-- Location: docs/ISSUE_1413_AUDIT.md — co-located with the rest of
     the project's durable documentation under docs/, mirroring
     docs/INCIDENT_RESPONSE.md, docs/RESERVE_ACCOUNTING.md, etc. -->

This file documents the evidence behind the docs-only PR that closes
[#1413](https://github.com/StellarLend/stellarlend-contracts/issues/1413)
("`amm: flash_swap_a_for_b declared to return i128 but its body returns
Result<i128, AmmPoolError>`"). It is committed on the same branch so
maintainers can read it from the PR's file diff directly, without
depending on PR-thread content (which the agent's integration token
could not edit on the upstream repo — see "Sandbox / CI caveat"
below).

## Audit

Snapshot taken **before** the PR's three doc-only commits, with
`stellar-lend/contracts/amm/src/lib.rs` at the HEAD of `flash/swap`
(commit `429fb5f6`). The two follow-up commits on the AMM rustdoc
(`b71cd854`, `a896e41e`) only extend an existing crate-level rustdoc
note; they introduce no `pub fn`, no `use`, no storage key, no type,
and no invariant.

```text
$ wc -l stellar-lend/contracts/amm/src/lib.rs
183 stellar-lend/contracts/amm/src/lib.rs

$ grep -RInE 'flash_swap_a_for_b|repay_flash_swap|AmmPoolError|assert_no_active_flash_swap' stellar-lend/contracts/amm
stellar-lend/contracts/amm/src/lib.rs:28: //! - Flash‑swap APIs ...
                (sole hit is the existing rustdoc note that the PR extends —
                 zero fn / struct / type / storage-key declarations anywhere)

$ grep -nE '^\s*pub fn' stellar-lend/contracts/amm/src/lib.rs
54:    pub fn init_pool(env: Env, a: i128, b: i128)
60:    pub fn add_liquidity(env: Env, add_a: i128, add_b: i128)
71:    pub fn remove_liquidity(env: Env, rem_a: i128, rem_b: i128)
86:    pub fn swap_a_for_b(env: Env, amount_in: i128, fee_bps: i128) -> i128
114:   pub fn get_reserves(env: Env) -> (i128, i128)
```

Conclusions:

- The symbol `flash_swap_a_for_b` is **not declared** anywhere in the
  AMM crate, so the report's claim of a return-type mismatch is *not
  applicable* on this branch.
- `AmmPoolError` is **not declared**, so no error type would need to
  be changed either.
- `repay_flash_swap` and `assert_no_active_flash_swap` are **not
  declared**, so no paired-API coupling on that side either.
- There are no flash-swap tests in the AMM crate, so no flash-swap
  test set whose pass/fail could be perturbed by this PR.

## Sandbox / CI caveat (transparency)

The original task explicitly said *"check that all tests pass"*. The
diff on this branch is doc-only — only `//!` lines added or
restructured in `stellar-lend/contracts/amm/src/lib.rs`, plus this
file. Any `cargo test -p stellarlend-amm` run on this branch returns
exactly the same result as on `main` (no behaviour, no imports, no
storage keys, no invariants changed).

The parent agent's execution sandbox does **not** have `cargo` /
`rustc` installed, so no `cargo test` was actually executed before
this PR was opened. Treat the **upstream CI** green-build signal on
this PR as the authoritative gate — see
[`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml) for
what the gate runs. If the upstream workflow rejects the PR for any
reason that depends on a missing test run, ping the issue author;
the change itself contains no executable code paths that would
change a test outcome.

## Closing #1413

Issue #1413 is closed by the PR via `closes #1413` because the
report's premise ("a return-type mismatch on `flash_swap_a_for_b`")
is provably **not applicable** on this branch — the function does not
exist. If reviewers prefer that #1413 be closed as
**not applicable / won't fix** via a maintainer comment rather than
the auto-close keyword, that closure mode is compatible with this PR:
the rationale is recorded in the rustdoc, in this file, and in the
PR description regardless.

## Future protocol change

When the flash-swap protocol change lands, the following must be
introduced. Exact shapes are to be decided by the protocol-change PR,
not pinned by this docs-only PR — the bug report is **not** a design
doc.

1. `flash_swap_a_for_b(env, amount_out: i128, params: Bytes) ->
   Result<i128, AmmPoolError>` — a *candidate* signature recorded in
   the issue; the landed signature may differ.
2. `repay_flash_swap(...)` paired with `assert_no_active_flash_swap(...)`
   — names also subject to the protocol-change PR.
3. An error type (`AmmPoolError` per the issue, or whatever the team
   chooses to name it — to be decided by the protocol-change PR).
4. A **regression test** for the flash-swap surface, since none
   currently exercises the unimplemented API.
