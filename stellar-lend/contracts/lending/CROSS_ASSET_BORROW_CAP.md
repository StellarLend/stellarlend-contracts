# Cross-Asset Borrow Cap

Rationale

A per-asset protocol borrow cap limits the total outstanding principal denominated
in a given asset. This bounds single-asset concentration risk (e.g., too much
of one borrowed asset outstanding) and complements existing per-asset debt
ceilings and the aggregate protocol debt accumulator.

Behavior

- `borrow_cap` is stored inside `AssetParams` (raw asset units). A value of `0`
  means "uncapped" and preserves current behaviour.
- At borrow time, after accruing interest on the borrower's position, the
  contract computes `total_debt_for(asset) + delta_principal` and rejects the
  borrow with `LendingError::BorrowCapExceeded` if that value would exceed the
  configured `borrow_cap` (and `borrow_cap != 0`).
- Repayments decrement the tracked per-asset total so capacity is reclaimable.
- All arithmetic is checked to avoid panics; numeric failures return
  `LendingError::Overflow`.

Worked example

1. Admin sets `borrow_cap = 1000` for asset A.
2. No current outstanding debt for asset A (total = 0).
3. User X borrows 800 units of A: allowed → total becomes 800.
4. User Y attempts to borrow 300 units of A: rejected with
   `BorrowCapExceeded` because 800 + 300 > 1000.
5. User X repays 200 units: total becomes 600. User Y can now borrow up to 400
   units.

Edge cases and notes

- `borrow_cap = 0` means uncapped — useful for legacy behaviour and for
  assets where the protocol does not want to impose a hard cap.
- The cap is enforced after accrual — interest that increases a user's
  principal can cause previously-available capacity to be consumed.
- The enforcement uses checked arithmetic and rolls back per-user changes on
  rejection so state remains consistent.
- This feature is additive: it does not replace existing per-asset `debt_ceiling`
  (a separate constraint) or overall protocol-level accounting.

See code: `src/cross_asset.rs` for the enforcement site and `src/lib.rs` for the
API change to `set_asset_params`.
