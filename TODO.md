# TODO: cross-asset health perf benchmark + read budget

- [ ] Analyze `compute_aggregate_health_factor` storage access pattern (confirm redundant reads)
- [ ] Implement trimmed-read optimization in `stellar-lend/contracts/lending/src/cross_asset.rs` (no behavior change)
- [ ] Add benchmark/budget regression test: `stellar-lend/contracts/lending/src/cross_asset_health_perf_test.rs`
- [ ] Add docs: `stellar-lend/contracts/lending/CROSS_ASSET_HEALTH_PERF.md`
- [ ] Wire test module in `stellar-lend/contracts/lending/src/lib.rs`
- [ ] Run `cargo test -p stellarlend-lending cross_asset_health_perf`
- [ ] If needed, update budget constants / formulas to match implementation

