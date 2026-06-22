# Reserve Feature Status

## Lending Reserve Factor

Implemented in `contracts/lending`.

| Capability | Status | Notes |
|---|---:|---|
| Admin reserve factor | Done | `set_reserve_factor_bps`, bounded to `[0, 5000]` bps |
| Reserve factor view | Done | `get_reserve_factor_bps` returns `0` when unset |
| Total reserve view | Done | `get_total_reserve` reads `DataKey::TotalReserve` |
| Borrow/repay accrual split | Done | Settled interest is split before principal is updated |
| Protocol-favor rounding | Done | Positive reserve splits use ceiling division |
| Reserve withdrawal | Not included | No withdrawal entrypoint exists in the lending contract yet |

## Accounting Rule

On each debt settlement:

```
interest = accrue_interest(principal, elapsed, rate_bps)
reserve_share = ceil(interest * reserve_factor_bps / 10_000)
principal += interest - reserve_share
total_reserve += reserve_share
```

The borrower still owes the full accrued interest. The reserve share is separated
into protocol accounting, while the remaining interest compounds into borrower
principal.

## Validation

Covered by `contracts/lending/src/reserve_factor_test.rs`:

- default zero reserve factor
- admin-set factor bounds
- repayment accrual split
- accumulation across multiple settlements
- max factor behavior
- tiny-interest rounding in the protocol's favor
