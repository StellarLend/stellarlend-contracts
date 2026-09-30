# Requirements Document

## Introduction

The StellarLend lending contract currently accrues interest per-position by re-computing elapsed-time compounding on every touch (`accrue_interest` / `settle_accrual`). This naive model forces each `DebtPosition` to carry its own `last_update` timestamp and cannot apply a protocol-wide rate change retroactively.

This feature replaces the per-position time-based accrual with the industry-standard **global borrow index** model:

- A single `DataKey::BorrowIndex` value is maintained in contract storage and updated lazily on each protocol touch.
- Each `DebtPosition` gains a `borrow_index_snapshot` field recording the index at the time of last interaction.
- A position's current debt is computed as `principal × current_index / snapshot_index`, making accrual O(1) and globally consistent.
- A migration function initialises existing positions' snapshots to the index at upgrade time.

---

## Glossary

- **BorrowIndex**: The single, monotonically-increasing global borrow index stored under `DataKey::BorrowIndex`. Scaled to `INDEX_SCALE` (10^7) fixed-point units.
- **INDEX_SCALE**: The canonical fixed-point scale for the borrow index. Defined as `10_000_000i128` (10^7), matching the protocol's 7-decimal internal standard.
- **DebtPosition**: The on-chain struct representing a borrower's debt state. After this feature it contains `principal`, `borrow_index_snapshot`, and `owner`.
- **borrow_index_snapshot**: The value of `BorrowIndex` at the time a `DebtPosition` was last touched. Used as the denominator in accrual calculations.
- **Protocol Touch**: Any contract invocation that reads or modifies debt state — borrow, repay, liquidate, accrue.
- **Rate_Model**: The existing interest-rate model that returns an annualised borrow rate given utilisation. Unchanged by this feature.
- **Accrual**: The process of computing how much interest has accumulated on a position since its last snapshot.
- **Migration**: The one-time upgrade operation that sets `borrow_index_snapshot` on all existing `DebtPosition` records that pre-date this feature.
- **Checked Arithmetic**: Arithmetic operations that return `Result` or call `checked_*` variants, never silently overflowing or wrapping.

---

## Requirements

### Requirement 1: Global Borrow Index Storage

**User Story:** As a protocol operator, I want a single global borrow index stored on-chain, so that all borrower positions are governed by one consistent interest accumulator.

#### Acceptance Criteria

1. THE `LendingContract` SHALL store the current borrow index under `DataKey::BorrowIndex` in instance storage, initialised to `INDEX_SCALE` at contract deployment.
2. THE `LendingContract` SHALL expose a read-only `get_borrow_index` function that returns the current stored `BorrowIndex` value without modifying state.
3. WHEN the contract is first deployed, THE `LendingContract` SHALL initialise `BorrowIndex` to `INDEX_SCALE` (representing a starting multiplier of 1.0).

---

### Requirement 2: Lazy Index Update on Protocol Touch

**User Story:** As a borrower, I want the global index to reflect up-to-date interest whenever I interact with the protocol, so that my accrued debt is calculated against a current rate.

#### Acceptance Criteria

1. WHEN any Protocol Touch occurs, THE `LendingContract` SHALL update `BorrowIndex` before reading or modifying any `DebtPosition`.
2. WHEN updating `BorrowIndex`, THE `LendingContract` SHALL compute the new index as `current_index × (1 + rate × elapsed_seconds / SECONDS_PER_YEAR)` using the `Rate_Model` output and the elapsed time since the last update, scaled to `INDEX_SCALE`.
3. WHEN the elapsed time since the last index update is zero seconds, THE `LendingContract` SHALL leave `BorrowIndex` unchanged.
4. WHILE `BorrowIndex` is being updated, THE `LendingContract` SHALL record the current ledger timestamp as the new `last_index_update` value in instance storage.

---

### Requirement 3: Per-Position Index Snapshot

**User Story:** As a borrower, I want my debt position to record the index value at my last interaction, so that my accrued interest can be computed in O(1) time using only the current and snapshot index values.

#### Acceptance Criteria

1. THE `DebtPosition` struct SHALL contain a `borrow_index_snapshot` field of type `i128` alongside `principal` and `owner`.
2. WHEN a new `DebtPosition` is created, THE `LendingContract` SHALL set `borrow_index_snapshot` to the current `BorrowIndex` value at the time of creation.
3. WHEN a `DebtPosition` is repaid or modified, THE `LendingContract` SHALL update `borrow_index_snapshot` to the current `BorrowIndex` after accrual is applied.
4. WHERE a `DebtPosition` carries a `borrow_index_snapshot` greater than the current `BorrowIndex` (a state that should not occur under normal operation but may arise from out-of-order migration), THE `LendingContract` SHALL treat the position's accrued interest as zero and SHALL NOT reduce `principal` below its stored value.

---

### Requirement 4: O(1) Accrual via Index Ratio

**User Story:** As a protocol integrator, I want debt accrual to be computable from the index ratio alone, so that the calculation cost is constant regardless of how long the position has been open.

#### Acceptance Criteria

1. WHEN computing the current debt of a `DebtPosition`, THE `LendingContract` SHALL calculate `current_debt = position.principal × current_index / position.borrow_index_snapshot` using `INDEX_SCALE`-aligned fixed-point arithmetic.
2. WHEN `position.borrow_index_snapshot` equals `current_index`, THE `LendingContract` SHALL return `position.principal` as the current debt unchanged.
3. THE `LendingContract` SHALL expose a `compute_debt` function that accepts a `DebtPosition` and returns the current debt without modifying state.
4. FOR ALL valid `DebtPosition` values where `borrow_index_snapshot <= current_index`, THE `LendingContract` SHALL produce a `current_debt >= position.principal` (interest is non-negative).

---

### Requirement 5: Index Monotonicity

**User Story:** As an auditor, I want the global borrow index to be monotonically non-decreasing, so that debt can never be retroactively reduced by an index movement.

#### Acceptance Criteria

1. FOR ALL sequences of Protocol Touches with non-negative elapsed time, THE `LendingContract` SHALL produce a `BorrowIndex` value that is greater than or equal to the previous `BorrowIndex` value.
2. IF a computed index update would result in a value less than the current `BorrowIndex`, THEN THE `LendingContract` SHALL retain the existing `BorrowIndex` value unchanged.
3. WHEN the borrow rate returned by `Rate_Model` is zero, THE `LendingContract` SHALL leave `BorrowIndex` unchanged.

---

### Requirement 6: Checked Arithmetic and Overflow Guards

**User Story:** As a security reviewer, I want all borrow index arithmetic to use checked operations, so that the contract never silently overflows or produces incorrect debt values.

#### Acceptance Criteria

1. THE `LendingContract` SHALL use only `checked_mul`, `checked_div`, and `checked_add` (or equivalent checked variants) for all `BorrowIndex` and debt accrual calculations.
2. IF a checked arithmetic operation on `BorrowIndex` would overflow `i128`, THEN THE `LendingContract` SHALL panic with a descriptive message rather than producing a wrapped or incorrect value.
3. IF a computed debt value from `compute_debt` would overflow `i128`, THEN THE `LendingContract` SHALL panic with a descriptive message.
4. THE `LendingContract` SHALL reject any `BorrowIndex` update that would cause the index to exceed `i128::MAX / INDEX_SCALE`, treating this as an overflow condition.

---

### Requirement 7: Migration of Existing Positions

**User Story:** As a protocol operator, I want a safe migration path that initialises all existing `DebtPosition` snapshots on upgrade, so that pre-existing borrowers are not disadvantaged or broken by the layout change.

#### Acceptance Criteria

1. THE `LendingContract` SHALL provide a `migrate_positions` admin function that iterates over all stored `DebtPosition` records and sets `borrow_index_snapshot` to the current `BorrowIndex` for any position where the snapshot is absent or set to zero.
2. WHEN `migrate_positions` is called, THE `LendingContract` SHALL require admin authorisation before modifying any position data.
3. WHEN `migrate_positions` is called, THE `LendingContract` SHALL first update `BorrowIndex` to reflect elapsed time before writing snapshots, so that all migrated positions share the same post-upgrade index baseline.
4. WHEN `migrate_positions` completes, THE `LendingContract` SHALL emit a `MigrationComplete` event recording the index value used and the number of positions migrated.
5. IF `migrate_positions` is called a second time after all positions have valid snapshots, THEN THE `LendingContract` SHALL perform no writes and return a count of zero.

---

### Requirement 8: Index Parser and Pretty-Printer (Serialisation Round-Trip)

**User Story:** As a developer integrating the API layer, I want the `BorrowIndex` value to serialise and deserialise correctly across the Soroban XDR boundary, so that off-chain tooling and the TypeScript API see a consistent value.

#### Acceptance Criteria

1. WHEN a `BorrowIndex` value is written to contract storage and then read back, THE `LendingContract` SHALL return the identical `i128` value with no loss or mutation.
2. FOR ALL valid `i128` borrow index values, encoding then decoding the value through Soroban's `contracttype` serialisation SHALL produce the original value (round-trip property).
3. THE TypeScript API layer SHALL deserialise `BorrowIndex` storage reads into a `bigint` without truncation for index values up to `i128::MAX`.
