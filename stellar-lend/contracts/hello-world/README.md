# `contracts/hello-world` — retired, not live code

**Nothing in this directory is built, linted, formatted, or tested.** Read this
before changing anything here.

## Status

| Fact | Where it is enforced |
|------|----------------------|
| Not a workspace member | `stellar-lend/Cargo.toml` (`[workspace] members`) |
| Not built by CI | `.github/workflows/ci-cd.yml` never names it |
| Excluded from the coverage gate | `stellar-lend/.tarpaulin.toml` |
| Does not compile | `cargo build` reports 90+ errors |

The crate is a stale, unmaintained near-duplicate of `contracts/lending`, left
over from an earlier repository layout. `src/lib.rs` diverges from
`contracts/lending/src/lib.rs` by roughly 5,400 lines, and most files under
`src/` mirror lending's `borrow` / `repay` / `liquidate` / `governance` /
`oracle` modules almost verbatim, each with its own accompanying docs.

It is retained on disk only so the history stays readable while it is
reconciled with, or replaced by, `contracts/lending`. It was removed from the
workspace rather than fixed; the sources were **not** deleted.

## The actively-maintained contract lives at `contracts/lending`

Do not add features or tests here. `contracts/lending` is the real lending
contract and is the one CI targets. In particular, the reentrancy-guard and
flash-loan repayment coverage that this crate's dead scaffolding was meant to
provide already exists there:

| Concern | Test file |
|---------|-----------|
| Reentrancy guard blocks every state-mutating op inside a flash-loan callback | `contracts/lending/tests/reentrancy_guard_test.rs` |
| Reentrancy during liquidation | `contracts/lending/tests/liquidate_reentrancy_test.rs` |
| Flash-loan repayment (exact / over / zero-fee / under-repayment rollback) | `contracts/lending/tests/flash_loan_repayment.rs` |
| Reverting and under-repaying callback rollback | `contracts/lending/tests/flash_callback_revert_test.rs` |
| Flash-loan max-utilization and pause gating | `contracts/lending/src/flash_utilization_test.rs`, `contracts/lending/src/flash_pause_gating_test.rs` |

Once that crate builds, they are run with:

```bash
cd stellar-lend
cargo test -p stellarlend-lending --test reentrancy_guard_test
cargo test -p stellarlend-lending --test flash_loan_repayment
cargo test -p stellarlend-lending --test flash_callback_revert_test
cargo test -p stellarlend-lending --test liquidate_reentrancy_test
```

> **Note:** as of this writing `contracts/lending` does not currently compile
> — `src/lib.rs` has unresolved merge-conflict markers committed to it, so
> `cargo build -p stellarlend-lending` fails before any test runs. That
> corruption is pre-existing and unrelated to this directory; see the note in
> `stellar-lend/Cargo.toml`.

## Known dead scaffolding still on disk

Because the crate is not a workspace member, `cargo` never type-checks it, so
its contents have rotted. The following are known-inert and are listed so they
are not mistaken for working code:

**Files present in `src/` but never declared by any `mod` statement** — Rust
only compiles a `.rs` file that some module declares, so these are invisible to
the module system and would never run even if the crate compiled:

- `src/borrow_isolation_tier_test.rs`
- `src/cross_asset_price_authorization_test.rs`
- `src/gov_payload_hash_test.rs`
- `src/position_summary_test.rs` (placeholder: a TODO list plus one `#[ignore]`d
  empty test)

**Truly trivial stubs that *are* declared** — effectively no implementation:

- `src/multisig.rs` (15 bytes; the entire file is the comment `// Stub module`),
  yet `lib.rs` calls `multisig::ms_set_admins`, `ms_propose_set_min_cr`,
  `ms_approve` and `ms_execute`.
- `src/analytics.rs` (310 bytes) — empty function bodies, yet `lib.rs` calls
  `analytics` symbols that do not exist (`get_protocol_stats`,
  `get_protocol_utilization`, `get_user_activity_summary`, `UserMetrics`,
  `ActivityEntry`, `ProtocolMetrics`).

**Not stubs, despite appearances.** Several modules that tracking issues
describe as "stubs" are in fact substantial implementations whose public API has
drifted away from the names `lib.rs` still calls (`borrow.rs`, `repay.rs`,
`withdraw.rs`, `deposit.rs`, `liquidate.rs`, `recovery.rs`, `config.rs`,
`config_snapshot.rs`, `flash_loan.rs`, `reserve.rs`, `risk_management.rs`).
`src/errors.rs` is a valid one-line re-export of
`crate::governance::GovernanceError`, and `src/storage.rs` is a real
`GuardianConfig` type. This is why reconciling the crate is not a matter of
filling in blanks, and why it was dropped from the workspace instead.

`src/test.rs` is a smoke test that only asserts `1u32 == 1u32`; it covers
nothing.

## Reporting issues here

Because nothing here compiles or runs, an issue against this crate cannot be
verified by CI. Check `git ls-files` before filing: a file that is not declared
by a `mod` statement in `src/lib.rs` is not part of the build. The stub-module
work items are tracked individually; prefer filing against
`contracts/lending` for anything intended to be maintained.
