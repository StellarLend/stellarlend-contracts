# TODO - Liquidation parameter setters + storage-backed reads

- [ ] Implement DataKey variants for liquidation threshold, close factor, and liquidation incentive (defaults to current constants)
- [ ] Add LendingError variants (or reuse existing) for invalid liquidation parameters
- [ ] Add admin-only setter functions + getters for these parameters
- [ ] Update `liquidate` to read threshold/close factor/incentive from storage (not hardcoded constants)
- [ ] Update `get_position` and `get_health_factor` to use storage-backed liquidation threshold
- [ ] Add/adjust unit tests to cover: defaults, setters bounds validation, and liquidation/health-factor behavior changes
- [ ] Update `stellar-lend/contracts/lending/README.md` and `docs/interface_quick_reference.md` to reflect new public API
- [ ] Run `cargo test -p stellarlend-lending`

