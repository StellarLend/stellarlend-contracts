# View Schema Versioning Policy

## Overview

This document defines the versioning policy for view functions in the StellarLend protocol, with particular focus on `get_user_position` and the `UserPositionSummary` struct. The policy ensures backwards compatibility for frontend integrations and protects against breaking changes during contract upgrades.

## Current Schema Version

- **Version**: 1
- **Status**: Stable
- **Last Updated**: Initial implementation

## Schema Stability Guarantees

### Field Name Stability
All field names in `UserPositionSummary` are **immutable**:
- `collateral_balance` - Raw collateral amount (i128)
- `collateral_value` - Collateral value in common unit (i128) 
- `debt_balance` - Total debt including interest (i128)
- `debt_value` - Debt value in common unit (i128)
- `health_factor` - Health factor scaled by 10000 (i128)

### Field Type Stability
All field types are **immutable**:
- Numeric fields remain `i128`
- No field type changes without schema version bump
- No field reordering (Soroban sorts XDR keys lexicographically)

### Serialization Contract
- `UserPositionSummary` serializes as XDR map keyed by field name
- Field order is lexicographically sorted by Soroban runtime
- Byte layout is deterministic and stable across versions
- Total serialized size: 80 bytes (5 fields × 16 bytes each)

## Versioning Rules

### When to Bump Version

**MAJOR BREAKING CHANGES (Required version bump):**
- Adding/removing fields from `UserPositionSummary`
- Changing field names
- Changing field types
- Changing field semantics (e.g., health factor scale)

**MINOR COMPATIBLE CHANGES (No version bump):**
- Internal implementation optimizations
- Bug fixes that preserve field values
- Performance improvements
- Documentation updates

### Version Bump Process

1. **Evaluate Impact**: Determine if changes affect public schema
2. **Update Version**: Increment `VIEW_SCHEMA_VERSION` constant
3. **Update Tests**: Add new schema version tests
4. **Migration Plan**: Document migration path for integrators
5. **Communication**: Notify frontend teams of breaking changes

## Backwards Compatibility Guarantees

### Contract-Level Guarantees

**For Schema Version 1:**
- All existing fields preserved indefinitely
- Field values maintain same semantics and scale
- Serialization format remains stable
- No field reordering or type changes

**For Future Versions:**
- Previous schema versions supported via versioned getters
- Migration path documented for each version bump
- Deprecation period minimum 6 months for breaking changes

### Integration-Level Guarantees

**Frontend Integration Support:**
- Schema version accessible via `VIEW_SCHEMA_VERSION` constant
- Version-specific getters available (e.g., `get_user_position_v1`)
- Migration examples provided in documentation
- Breaking changes communicated via governance proposals

**API Stability:**
- Function signatures remain stable
- Error handling patterns preserved
- Return value formats consistent within version
- No silent failures or data corruption

## Testing Requirements

### Schema Stability Tests

All view schema changes must include:

1. **Serialization Tests**: Verify byte-for-byte serialization stability
2. **Field Order Tests**: Confirm field ordering unchanged
3. **Value Consistency Tests**: Ensure field values preserved
4. **Upgrade Tests**: Test consistency across contract upgrades
5. **Edge Case Tests**: Test boundary conditions and large values

### Coverage Requirements

- Minimum 95% test coverage for view functions
- All upgrade scenarios tested
- All field boundary conditions covered
- Schema version constants tested

## Migration Guidelines

### For Frontend Integrators

**Upgrading to Schema Version 1:**
- No action required - current schema is version 1
- Use `VIEW_SCHEMA_VERSION` constant to detect version
- Implement defensive programming for future versions

**Preparing for Future Versions:**
- Monitor `VIEW_SCHEMA_VERSION` constant
- Implement version-specific handling
- Test against upgrade scenarios
- Plan migration path for breaking changes

### For Contract Developers

**Adding New Fields:**
1. Create new struct: `UserPositionSummaryV2`
2. Implement new getter: `get_user_position_v2()`
3. Update version constant to 2
4. Add comprehensive tests
5. Document migration path

**Maintaining Compatibility:**
- Preserve existing getters unchanged
- Add versioned getters for new schemas
- Provide migration utilities
- Document deprecation timeline

## Implementation Details

### Current Schema (Version 1)

```rust
#[contracttype]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserPositionSummary {
    /// User's collateral balance (raw amount)
    pub collateral_balance: i128,
    /// Collateral value in common unit (e.g. USD 8 decimals)
    pub collateral_value: i128,
    /// User's debt balance (principal + accrued interest)
    pub debt_balance: i128,
    /// Debt value in common unit
    pub debt_value: i128,
    /// Health factor scaled by 10000 (10000 = 1.0)
    pub health_factor: i128,
}

pub const VIEW_SCHEMA_VERSION: u32 = 1;
```

### Serialization Format

- **Total Size**: 80 bytes
- **Field Order**: Lexicographically sorted by Soroban
- **Byte Layout**: Each i128 field = 16 bytes big-endian
- **Deterministic**: Same input always produces same output

### Version Detection

```rust
// Check current schema version
let current_version = views::VIEW_SCHEMA_VERSION;

// Version-specific getter usage
match current_version {
    1 => position = client.get_user_position_v1(&user),
    2 => position = client.get_user_position_v2(&user),
    _ => panic!("Unsupported schema version"),
}
```

## Security Considerations

### View Function Security

- **Read-Only**: All view functions are read-only
- **No State Changes**: Views cannot modify contract state
- **Oracle Dependencies**: Value fields depend on oracle configuration
- **Consistency**: Views must agree with underlying storage

### Upgrade Security

- **State Preservation**: User positions preserved across upgrades
- **Schema Stability**: Serialization format stable within version
- **Rollback Safety**: Positions survive upgrade rollbacks
- **Migration Safety**: No data loss during schema migrations

## Governance Process

### Schema Changes

All schema version changes require:

1. **Proposal**: Governance proposal detailing changes
2. **Review**: Security review and impact assessment
3. **Testing**: Comprehensive test suite validation
4. **Vote**: Community approval of changes
5. **Implementation**: Coordinated upgrade with migration

### Communication

- **Advance Notice**: Minimum 30 days notice for breaking changes
- **Documentation**: Updated docs and migration guides
- **Support**: Developer assistance for migration
- **Monitoring**: Post-upgrade monitoring for issues

## References

- [Views Implementation](src/views.rs)
- [View Tests](src/views_test.rs)
- [Upgrade Migration Tests](src/upgrade_migration_safety_test.rs)
- [View Upgrade Consistency Tests](src/view_upgrade_consistency_test.rs)
- [Storage Documentation](docs/storage.md)

---

**Last Updated**: 2026-04-29  
**Next Review**: 2026-10-29  
**Maintainer**: StellarLend Protocol Team
