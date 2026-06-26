# Event Schema Versioning

## Overview

StellarLend emits a stable, versioned event schema so that indexers and
integrators can decode events safely across contract upgrades.

The versioning strategy is minimal by design:

- A single `EVENT_SCHEMA_VERSION: u32` constant in
  `contracts/hello-world/src/events.rs` is the single source of truth.
- Versioned event structs carry a `schema_version: u32` field populated with
  that constant at emit time.
- A `SchemaVersionEvent` is emitted once during `initialize`, giving indexers
  an on-chain anchor for the version active at deployment.

---

## Versioned Events

| Struct | Since version | Notes |
|---|---|---|
| `SchemaVersionEvent` | 1 | Emitted once on `initialize`. |
| `LiquidationEventV1` | 1 | Versioned liquidation with post-liquidation borrower snapshot. |
| `BorrowerHealthEventV1` | 1 | Borrower health snapshot emitted alongside position updates. |
| `DepositEvent` | 1 | Versioned deposit event with user and new collateral balance. |
| `WithdrawEvent` | 1 | Versioned withdraw event with user and new collateral balance. |
| `BorrowEvent` | 1 | Versioned borrow event with user and new debt balance. |
| `RepayEvent` | 1 | Versioned repay event with user and new debt balance. |
| `AmmSwapEventV1` | 1 | Versioned AMM swap event with stable `amm/v1` topics. |
| `AmmLiquidityAddedEventV1` | 1 | Versioned AMM add-liquidity event with stable `amm/v1` topics. |
| `AmmLiquidityRemovedEventV1` | 1 | Versioned AMM remove-liquidity event with stable `amm/v1` topics. |

## AMM Event Topics

AMM mutation events must publish a stable, versioned topic tuple:

```rust
(env.events().publish(
    (Symbol::new(&env, "amm"), Symbol::new(&env, "v1"), Symbol::new(&env, "swap")),
    event_data
));
```

Supported AMM event kinds for version `v1`:

- `swap`
- `add_liquidity`
- `remove_liquidity`

Each versioned AMM event payload must include `schema_version: 1` and an
explicit `event` field that matches the final topic segment.

All other events are unversioned (no `schema_version` field). They follow an
additive-only policy: new fields may be appended but existing fields will not
be removed or reordered within a major version.

---

## Decoding Guide for Indexers

### Step 1 – Anchor the version on deployment

When a new contract instance is deployed, the first event emitted is
`SchemaVersionEvent`. Persist `version` from this event alongside the contract
address.

```json
{
  "event_name": "SchemaVersionEvent",
  "schema_version": 1,
  "timestamp": 1714176000
}
```

### Step 2 – Read `schema_version` from every versioned event

For events that carry a `schema_version` field, always read it before
decoding the rest of the payload:

```python
def decode_event(raw):
    version = raw.get("schema_version")
    if version == 1:
        return decode_v1(raw)
    elif version == 2:
        return decode_v2(raw)
    else:
        raise UnknownSchemaVersion(version)
```

### Step 3 – Handle `None` / absent `schema_version`

Events without a `schema_version` field are legacy / unversioned events.
Decode them using the field set documented at the time of the contract version
you are indexing.

---

## Upgrade Policy

### Additive changes (no version bump required)

- Adding a new **unversioned** event struct.
- Appending optional fields to an existing unversioned event (indexers must
  tolerate unknown fields).

### Version bump required

- Adding or removing a field on a **versioned** event struct.
- Changing the type of any field on a versioned event struct.

**Procedure for a breaking change:**

1. Increment `EVENT_SCHEMA_VERSION` in `events.rs`.
2. Introduce a new struct (e.g. `FooEventV2`) with the updated schema.
3. Emit **both** the old and new struct for one upgrade cycle so indexers can
   migrate without downtime.
4. After all known indexers have migrated, retire the old struct in the
   following upgrade.
5. Update this document and the table above.

### Example – adding a field to `LiquidationEventV1`

```rust
// Before (version 1)
pub struct LiquidationEventV1 {
    pub schema_version: u32,
    // ... existing fields
}

// After (version 2) – introduce V2, keep V1 for one cycle
pub struct LiquidationEventV2 {
    pub schema_version: u32,
    // ... existing fields
    pub new_field: i128,   // ← new
}
```

Bump `EVENT_SCHEMA_VERSION` to `2` and emit both `LiquidationEventV1` and
`LiquidationEventV2` during the transition cycle.

---

## Indexer Model

The `indexing_system` crate surfaces `schema_version` as an optional field on
both `CreateEvent` and `Event`:

```rust
pub struct CreateEvent {
    // ...
    /// None for unversioned events; Some(n) for versioned events.
    pub schema_version: Option<u32>,
}
```

The parser (`indexing_system/src/parser.rs`) automatically extracts
`schema_version` from the decoded JSON payload when the field is present.

---

## References

- `contracts/hello-world/src/events.rs` – `EVENT_SCHEMA_VERSION` constant,
  `SchemaVersionEvent`, `emit_schema_version`.
- `contracts/hello-world/src/tests/events_test.rs` –
  `test_schema_version_event_emitted`,
  `test_versioned_events_carry_current_schema_version`.
- `indexing_system/src/models.rs` – `CreateEvent.schema_version`,
  `Event.schema_version`.
- `indexing_system/src/parser.rs` – automatic extraction of `schema_version`.
- `docs/storage.md` – storage layout and migration strategy.
- `docs/UPGRADE_AUTHORIZATION.md` – upgrade authorization boundaries.
