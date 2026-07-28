# StellarLend Oracle Service

Off-chain oracle integration service that fetches price data from multiple external sources and updates the smart contract on Soroban.

## Features

- **Multi-Source Price Fetching**: Aggregates prices from CoinGecko and Binance
- **Price Validation**: Validates prices for staleness, deviation, and bounds
- **Weighted Median**: Calculates weighted median from multiple sources for accuracy
- **MAD Outlier Rejection**: Filters rogue/broken feed prices before aggregation using Median Absolute Deviation
- **Efficient Caching**: In-memory caching with configurable TTL to reduce API calls

## Prerequisites

- Node.js >= 18.0.0
- npm

## Installation

```bash
cd oracle
npm install
```

## Configuration

Copy the example environment file and configure:

```bash
cp .env.example .env
```

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `STELLAR_NETWORK` | Network: `testnet` or `mainnet` | Yes |
| `STELLAR_RPC_URL` | Soroban RPC endpoint | Yes |
| `CONTRACT_ID` | StellarLend contract address | Yes |
| `ADMIN_SECRET_KEY` | Secret key for signing transactions | Yes |
| `COINGECKO_API_KEY` | CoinGecko Pro API key | No |
| `CACHE_TTL_SECONDS` | Cache TTL in seconds (default: 30) | No |
| `UPDATE_INTERVAL_MS` | Price update interval (default: 60000) | No |
| `MAX_PRICE_DEVIATION_PERCENT` | Max price deviation % (default: 10) | No |
| `MAD_Z_SCORE_THRESHOLD` | MAD outlier filter z-score (default: 3.5, 0 = disabled) | No |
| `LOG_LEVEL` | Logging: debug, info, warn, error | No |

## Usage

### Development

```bash
npm run dev
```

### Production

```bash
npm run build
npm start
```

### Admin reload endpoint

The oracle service can expose a secure admin endpoint when `ADMIN_API_PORT` is configured.
Requests to `POST /reload-config` must include an `x-signature` header containing a hex HMAC-SHA256 over the raw request body using `ADMIN_HMAC_SECRET`.
The payload may include `validatorConfig` updates and/or asset-specific `bounds` to tighten min/max price ranges.

Example payload:

```json
{
  "bounds": {
    "XLM": { "minPrice": 0.1, "maxPrice": 1000000 }
  }
}
```

This endpoint is intended for emergency tightening of bounds without restarting the service.

### Testing

```bash
npm test                 # Run all tests
npm run test:coverage    # With coverage report
npm run test:watch       # Watch mode
```

## Live Integration Test

To verify proper operation with real APIs (CoinGecko, Binance), run the live test script:

```bash
npx tsx tests/live-test.ts
```

This script will:
1. Initialize the CoinGecko and Binance providers.
2. Fetch live prices for XLM and BTC from each.
3. Aggregate the prices and display the result.

## Supported Assets

| Asset | CoinGecko | Binance |
|-------|-----------|---------|
| XLM   | Yes       | Yes     |
| USDC  | Yes       | Yes     |
| BTC   | Yes       | Yes     |
| ETH   | Yes       | Yes     |
| SOL   | Yes       | Yes     |

## MAD Outlier Rejection

Before the weighted-median is computed, `filterOutliersByMAD` removes prices from
broken or malicious feeds using the **Median Absolute Deviation** method.

### Algorithm

For a set of scaled bigint prices `p₁ … pₙ`:

1. Compute the sample median `M`.
2. Compute `MAD = median(|pᵢ − M|)`.
3. Compute the modified z-score for each price:
   ```
   zᵢ = |pᵢ − M| / (1.4826 × MAD)
   ```
   The constant `1.4826` makes MAD a consistent estimator of σ under Gaussian noise.
4. Reject any price where `zᵢ > zMax`.

If filtering would leave fewer sources than `minSources`, the full unfiltered list is used as a safe fallback so the oracle never silently stalls.

### When filtering is skipped

- **≤ 2 prices** — not enough data to distinguish signal from noise; all prices are kept.
- **MAD = 0** — all prices are identical; nothing to reject.
- **zMax ≤ 0** — filtering is explicitly disabled.

### Configuration

| Parameter | Env var | Default | Effect |
|-----------|---------|---------|--------|
| `madZScoreThreshold` | `MAD_Z_SCORE_THRESHOLD` | `3.5` | Prices with modified z-score above this are rejected. Lower = stricter. |

A threshold of **3.5** is the value recommended by Iglewicz & Hoaglin (1993) for detecting outliers in small samples. For tighter protection lower it to `2.5`; set to `0` to disable entirely.

### Example

With prices `[100, 101, 102, 5000]` and `zMax = 3.5`:

- Median = 101.5, MAD = 1, modified z-score of 5000 ≈ 3290 → **rejected**.
- Output: `[100, 101, 102]`

## Price Sources

### CoinGecko (Primary)
- Popular crypto price API
- Priority: 1, Weight: 60%

### Binance (Secondary)
- Public market data API
- Priority: 2, Weight: 40%
- Exposes 24-hour quote volume (`quoteVolume` from `/api/v3/ticker/24hr`), which is used as the
  aggregation weight when available (see **Volume-Weighted Median** below).

## Volume-Weighted Median

The aggregator uses a weighted-median algorithm to combine prices from multiple sources.
The weight assigned to each price point follows this priority:

| Priority | Condition | Weight used |
|----------|-----------|-------------|
| 1 | `volume24h` is present and `> 0` | `Number(volume24h)` – 24 h quote volume in USD |
| 2 | `volume24h` is absent or zero | Static `provider.weight` from `ProviderConfig` |

**Why volume?**  
Liquid pairs (high volume) are harder to manipulate and track true market prices more
accurately. By weighting prices by 24 h volume, thin or illiquid pairs automatically
contribute less to the aggregated price during volatility, without any manual tuning.

**Mixing sources:**  
When some providers supply volume and others do not, both weight types can coexist in
the same aggregation round. Volume weights are typically many orders of magnitude larger
than static weights (e.g. `2_000_000` vs `0.4`), so any provider that supplies a
meaningful volume will effectively dominate the median. Providers without volume data
retain their relative influence through their static `weight` setting.

## Programmatic Usage

```typescript
import { OracleService, loadConfig } from 'stellarlend-oracle';

const config = loadConfig();
const service = new OracleService(config);

// Start automatic updates
await service.start(['XLM', 'USDC', 'BTC']);

// Or fetch manually
const price = await service.fetchPrice('XLM');

// Stop service
service.stop();
```

## Project Structure

```
oracle/
├── src/
│   ├── index.ts              # Main entry point
│   ├── config.ts             # Configuration
│   ├── providers/            # Price providers
│   │   ├── coingecko.ts      # CoinGecko API
│   │   └── binance.ts        # Binance API
│   ├── services/             # Core services
│   │   ├── price-validator.ts
│   │   ├── price-aggregator.ts
│   │   ├── cache.ts
│   │   └── contract-updater.ts
│   ├── types/                # TypeScript types
│   └── utils/                # Utilities
├── tests/                    # Test suites
└── package.json
```

## Logging Policy

To prevent operational metadata leakage in shared log aggregators, the oracle service never emits the raw admin public key to any log sink.

### Admin key redaction

- **One-time startup line**: On initialization, `ContractUpdater` logs `adminKeyPrefix` — a short SHA-256 prefix of the admin public key in the form `sha256:<first-8-hex-chars>` (e.g. `sha256:a1b2c3d4`). This is enough for an operator to confirm which key is active without exposing the full key.
- **Retry and error paths**: No logger call in the retry loop or error handler references the raw public key. Only the asset name, attempt number, and error message are included.
- **Helper function**: `hashPublicKey(pubkey: string): string` in `src/utils/logger.ts` is the single, tested point of contact for producing safe key identifiers. Use it for any future log site that needs to reference a Stellar public key.

### Rationale

A full Stellar G-address appearing in every retry log allows anyone with read access to a shared log aggregator to trivially correlate oracle failure windows with the deployer identity. The SHA-256 prefix retains enough entropy for operator correlation while making such correlation impossible without the original key.

## Cheers!

## Network Resiliency & Backoff Strategy

To mitigate thundering-herd issues during transient Soroban RPC node congestion, the oracle instance employs an **Linear/Exponential Backoff with Full Jitter** strategy on transaction retries.

Instead of a fixed interval, the wait time before any retry sequence is evaluated dynamically using the following equation:

$$delay = \text{jitter}(\min(\text{backoffCapMs}, \text{backoffBaseMs} \times 2^{\text{attempt}}))$$

### Config Knobs
These default settings can be overridden in your workspace environment configuration layout:
* `backoffBaseMs`: The initial delay seed multiplier (Default: `1000ms`).
* `backoffCapMs`: The maximum delay timeout ceiling across all cumulative attempts (Default: `10000ms`).

## Rate-Limit (429) Handling & Cooldown Semantics

To prevent spamming external endpoints when rate limited, the oracle service implements robust **Retry-After** header parsing and active provider cooldowns.

### Algorithm & Behavior

When a price provider returns an HTTP `429 Too Many Requests` status:
1. **Header Inspection**: The provider inspects the `Retry-After` header returned by the server.
2. **Retry-After Parsing**:
   - **Numeric duration (seconds)**: parsed as a non-negative integer and converted to milliseconds (e.g. `Retry-After: 30` -> 30,000ms cooldown).
   - **HTTP-Date (GMT Date-String)**: parsed into a timestamp using standard date parsing (e.g. `Retry-After: Fri, 31 Dec 1999 23:59:59 GMT`), setting the cooldown to expire at that specific time.
   - **Missing/Invalid Header**: Falls back to a standard **60-second** (60,000ms) cooldown to protect the API endpoint from immediate retries.
3. **Suspension State**: The provider's internal state sets `cooldownUntil` to the calculated timestamp, rendering `isCooledDown` true.
4. **Fetch Skipping**: While a cooldown is active, any direct calls to `fetchPrice` or `fetchPrices` on the provider are skipped and reject immediately without firing any HTTP requests.
5. **Aggregator Fallback**: The `PriceAggregator` surfaces this cooldown, automatically skipping any cooled-down provider and immediately falling back to alternative healthy providers (e.g., Binance) to carry the price aggregation load seamlessly.

