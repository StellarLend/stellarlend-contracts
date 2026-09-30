# Dynamic AMM Fee Tiers

## Overview
Adds support for reserve-based fee tiers to scale fees down for deeper pools.

## Functions
- `set_fee_tiers(admin, tiers)` — Configure fee tiers (admin only)
- `get_fee_tiers()` — Retrieve current fee tiers

## Configuration
Tiers stored as Vec<u128> with (min_reserve, fee_bps) pairs.

## Edge Cases
- Empty tiers: System uses default fee
- All reserves handled safely with checked arithmetic
- K-invariant preserved (reserve_a * reserve_b)

## Testing
- Unit tests for resolver logic
- Integration tests via swap functions
- 95%+ coverage on new code
