# Fee Tiers Decision

## Issue

The AMM crate had two free functions `set_fee_tiers` and `get_fee_tiers` (lines 1883-1896 of lib.rs) that were defined outside the `#[contractimpl]` block. Per the deployment notes, these functions were not contract entrypoints and any test expecting them on the client would fail.

## Analysis

1. **No usage**: Neither function was called anywhere in the codebase
2. **Not entrypoints**: Being free functions outside `#[contractimpl]`, they could not be invoked as contract methods
3. **Incomplete feature**: The fee tier logic was not integrated into the swap functions
4. **Storage key defined but unused**: `FEE_TIERS_KEY` constant was defined but only used in these two functions

## Decision

**Deleted both functions and the unused constant** for the following reasons:

1. **Scaffolding**: These functions appear to be incomplete scaffolding for a dynamic fee feature that was never fully implemented
2. **Compilation**: Removing them eliminates dead code and reduces confusion about the contract's actual API
3. **Future work**: If dynamic fees are needed later, they should be:
   - Properly added to the `#[contractimpl]` block as contract entrypoints
   - Integrated into the swap logic to actually use the fee tiers  
   - Protected with admin authorization (the `require_auth()` call was correct)
   - Tested with the rest of the contract

## Alternative Considered

Moving the functions into the `#[contractimpl]` block was considered, but rejected because:
- The fee tier logic is not integrated into swap functions
- No tests exist for this feature
- The data structure (`Vec<u128>`) would need to be properly designed for the tier lookup logic
- Adding an untested, unintegrated feature just to make it compile would violate the issue requirement: "Do not weaken assertions to make tests pass; if behaviour is wrong, open an issue"

## Related Code

The AMM currently uses a single stored fee (see `stored_fee_test.rs`) via `set_fee_bps` and `get_fee_bps`, which ARE proper contract entrypoints. The fee tier feature would have been an enhancement to this, allowing fees to scale based on reserve sizes.

## Status

Fee tiers remain an open feature request. If implemented, they should:
1. Use a proper storage structure (not just `Vec<u128>`)
2. Define the tier structure clearly (currently `FeeTier` struct exists with `min_reserve` and `fee_bps` fields)
3. Integrate into `compute_swap_out` to select the appropriate fee based on current reserves
4. Have comprehensive tests including edge cases (empty tiers, overlapping ranges, etc.)
