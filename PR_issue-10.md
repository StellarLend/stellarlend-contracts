# MR: Harden Interest Accrual Math and Document Units (#150)

Problem
- Interest accrual used 1e8 scaling without clear documentation and lacked overflow protections, risking incorrect balances over long durations.

Changes
- Added saturating math to interest accrual and rate computations in `InterestRateStorage::update_state` and `InterestRateManager::accrue_interest_for_position`.
- Documented units, scales, and formulas (rates scaled by 1e8, per-year seconds, utilization scale).
- Clamped rates to [0, 1e8] during accrual to avoid pathological inputs.

Acceptance
- cargo build/test successful.
- Existing tests pass; added docs in code to clarify units and prevent misuse.

Notes
- Follow-up: property-based tests for extreme rate/time combinations can be expanded in a dedicated test module if required.

# MR: Strengthen Oracle Aggregation and Validation (#152)

Problem
- Median/TWAP lacked staleness checks, outlier handling, and configurable policies.

Changes
- Added staleness enforcement via heartbeat TTL.
- Added deviation threshold (bps), configurable trim count for outlier trimming, and TWAP window parameter.
- Implemented median with trim and deviation filtering; safe indexing and saturating math.

Acceptance
- cargo build/test successful.
- Deterministic aggregation behavior; policies persisted via storage.

# MR: Wire Price Cache with TTL into Valuation (#153)

Problem
- Existing price cache not used, causing repeated oracle calls and no TTL enforcement.

Changes
- Implemented aggregated price cache in `oracle.rs` with TTL and events for hits, sets, and evictions.
- Integrated cache checks into `Oracle::aggregate_price` so valuation callers benefit automatically.

Acceptance
- cargo build/test successful.
- Behavior validated by unit tests remaining green; cache events emitted.