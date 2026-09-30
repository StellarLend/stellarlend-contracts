# StellarLend Hello-World Contract WASM Audit Status

## Status

- `contracts/hello-world` is a legacy contract directory that is not part of the active `stellar-lend` workspace.
- As of 2026-07-29, the `hello-world` crate is not compilable from the current source tree.
- No current `hello_world.wasm` build file or audit report exists for this crate.

## Important Caveat

This document intentionally replaces a stale WASM audit report with a dated non-current status notice. Any previously published `WASM Hash`, `WASM Size`, or exported-function counts for `hello-world` were generated from an older historical build and do not reflect the current repository state.

## Regeneration Instructions

Once the `hello-world` crate is restored and can build successfully again:

1. Build the contract with the appropriate Soroban command, such as `stellar contract build`.
2. Inspect the resulting WASM artifact with `stellar contract inspect`.
3. Update this file with the actual `WASM Size`, `WASM Hash`, and exported function summary.
4. Remove this status notice and replace it with the regenerated current audit report.

## Rationale

The current repository intentionally excludes `contracts/hello-world` from the active workspace because it is a legacy monolith with a wider API surface than the canonical deployment targets. Retaining this status note prevents the stale audit from being mistaken for a current build artifact.
