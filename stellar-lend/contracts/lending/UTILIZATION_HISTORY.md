# Utilization History

The lending contract exposes the current protocol utilization through
`get_protocol_metrics`, but clients also need a short trend for dashboards,
alerts, and rate-model inspection. Indexing every ledger is expensive for those
callers, so the contract keeps a bounded history of recent utilization samples.

## Storage Model

Samples are stored as `UtilizationSample { ledger, utilization_bps }` under
`DataKey::UtilizationHistory`. The internal vector is ordered oldest-first and
has a fixed capacity of `UTILIZATION_HISTORY_CAPACITY` entries.

When a new sample is written and the vector is full, the contract removes index
`0` and appends the new sample. This mirrors the AMM TWAP snapshot policy's
bounded-vector eviction shape: storage rent is capped, the newest sample is
always retained, and the cost of the shift is bounded by a constant capacity.

## Write Path

The contract writes a sample from the existing borrow-rate cache miss path:

1. `cached_borrow_rate` checks the temporary per-ledger cache.
2. On a miss, it loads `TotalDebt`, `TotalDeposits`, and `RateParams`.
3. It computes `utilization_bps = total_debt * 10_000 / total_deposits`.
4. It writes one utilization sample for the current ledger.
5. It stores the computed borrow rate in the temporary cache.

Warm reads in the same ledger reuse the cached rate and do not append duplicate
samples. No extra public entrypoint is needed to trigger writes.

## Read Path

`get_utilization_history()` returns a Soroban `Vec<UtilizationSample>` ordered
newest-first, which is the shape charting clients usually need. If no rate
update has happened yet, it returns an empty vector.

## Worked Example

Assume the capacity is above the number of observations shown here:

| Ledger | Total Debt | Total Deposits | Utilization |
| ------ | ---------- | -------------- | ----------- |
| 100    | 1,000      | 10,000         | 1,000 bps   |
| 101    | 7,500      | 10,000         | 7,500 bps   |
| 102    | 10,000     | 10,000         | 10,000 bps  |

The internal storage is oldest-first:

```text
[
  { ledger: 100, utilization_bps: 1000 },
  { ledger: 101, utilization_bps: 7500 },
  { ledger: 102, utilization_bps: 10000 },
]
```

The view returns newest-first:

```text
[
  { ledger: 102, utilization_bps: 10000 },
  { ledger: 101, utilization_bps: 7500 },
  { ledger: 100, utilization_bps: 1000 },
]
```

## Edge Cases

- Empty history returns an empty vector, not a panic.
- Zero total supply records `0` utilization.
- Same-ledger repeated rate reads write at most one sample because the rate cache
  is keyed by ledger sequence.
- Capacity is fixed. Once full, each new sample evicts exactly one oldest sample.
- Utilization arithmetic uses checked multiplication and division. An overflow
  while computing `total_debt * 10_000` is surfaced instead of silently wrapping.
