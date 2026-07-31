# Issue #1713: Fix set_fee_tiers/get_fee_tiers dead code

## Steps
- [x] Step 1: Remove `FeeTier` struct, `FEE_TIERS_KEY`, `set_fee_tiers`, `get_fee_tiers`, and duplicate `mod dynamic_fee_test` from `lib.rs`
- [x] Step 2: Replace `dynamic_fee_test.rs` placeholder with a doc-only file explaining the removal
- [x] Step 3: Update `DYNAMIC_FEE.md` to reflect removal of fee tiers and document `set_fee_bps`/`get_fee_bps` instead
- [ ] Step 4: Verify compilation (`cargo build`)
- [ ] Step 5: Verify tests pass (`cargo test`)

## Summary of changes
### lib.rs
- Removed unused `FeeTier` struct
- Removed unused `FEE_TIERS_KEY` constant
- Removed unused `Symbol` from imports
- Removed `set_fee_tiers()` and `get_fee_tiers()` free functions (dead code outside `#[contractimpl]`)
- Removed duplicate `mod dynamic_fee_test;` at bottom of file (only one kept at top)

### dynamic_fee_test.rs
- Replaced placeholder test with documentation comment explaining removal

### DYNAMIC_FEE.md
- Rewritten to document the actual fee management API (`set_fee_bps`/`get_fee_bps`)
- Includes implementation details, edge cases, and rationale for removal
