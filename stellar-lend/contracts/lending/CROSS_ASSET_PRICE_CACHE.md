# Cross-Asset Price Cache

## Overview
The `compute_aggregate_health_factor` function evaluates a user's cross-asset positions by computing their weighted collateral and comparing it to their total effective debt. Because prices are necessary for both sides of the evaluation, an asset present in both the user's collateral list and debt list would historically require its price record to be read from persistent storage twice in the same pass.

To eliminate this redundant storage read and reduce ledger fees, a local `Map<Address, PriceRecord>` cache is built at the top of the function.

## Rationale
In Soroban, persistent storage reads are metered and contribute to the transaction fee. A user could deposit an asset (like XLM) as collateral and subsequently borrow it (or maintain a previously borrowed position). During `compute_aggregate_health_factor`, reading the same `OraclePrice` entry in both loops without caching creates linear O(N+M) read costs.

By caching the `PriceRecord` upon its first fetch, we bound the number of price reads to strictly the number of *unique* assets the user holds, saving a constant but meaningful margin when assets overlap. This strictly reduces the worst-case bound for a user with $N$ collateral assets and $M$ debt assets from $2 + 3N + 2M$ reads to $2 + 3N + 2M - C$, where $C$ is the number of overlapping assets.

## Worked Example
Consider a user with XLM and USDC in their collateral list, and USDC and ETH in their debt list. `USDC` appears in both.

### State
- **XLM** (Collateral): Amount 500
- **USDC** (Collateral): Amount 100
- **USDC** (Debt): Amount 50
- **ETH** (Debt): Amount 1

### Pass Execution
**1. Setup**
- `price_cache` initialized as empty `Map`.

**2. Collateral Loop**
- **XLM**: Cache miss. Fetches from persistent storage. Cache is updated: `{ XLM -> PriceRecord(XLM) }`.
- **USDC**: Cache miss. Fetches from persistent storage. Cache is updated: `{ XLM -> PriceRecord(XLM), USDC -> PriceRecord(USDC) }`.

**3. Debt Loop**
- **USDC**: Cache hit! The `PriceRecord` for USDC is pulled directly from the local `price_cache`. No persistent storage read is issued.
- **ETH**: Cache miss. Fetches from persistent storage. Cache is updated: `{ XLM, USDC, ETH -> PriceRecord(ETH) }`.

The total number of price-related persistent reads is 3 instead of 4.

## Edge Cases

### Overlapping Asset
As shown above, the asset is fetched once on its first appearance (which is always the collateral loop, since it runs first) and hits the cache in the debt loop.

### Single-Sided Asset
If an asset appears in only the collateral list or only the debt list, it misses the cache, fetches the `PriceRecord` from persistent storage, and is added to the cache. The performance profile is identical to the pre-cache implementation (1 read).

### Empty Debt List
If the user's debt list is empty, the function returns the `HEALTH_FACTOR_NO_DEBT` sentinel immediately, without executing the collateral loop. The cache is never initialized or used, and 0 price reads occur.

### Stale Prices
The helper function `get_price_for_asset` performs a staleness check against `DEFAULT_ORACLE_MAX_AGE_SECS` *before* returning the `PriceRecord`. The cache stores the `PriceRecord` *only after* that check has passed.
- A stale price errors on the first fetch for that asset and the cache is never populated for it. The transaction immediately rolls back.
- A cache hit on the second occurrence does not re-run the staleness check — it reuses the already-validated record. Since both checks happen within the same invocation of the same contract (in the exact same ledger ledger sequence and timestamp), a price that was valid in the collateral loop is mathematically guaranteed to still be valid in the debt loop.
