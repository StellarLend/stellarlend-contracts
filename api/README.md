# StellarLend REST API

REST API for StellarLend core lending operations (deposit, borrow, repay, withdraw) with Stellar Horizon and Soroban RPC integration.

## Features

- REST endpoints for deposit, borrow, repay, withdraw operations
- Request validation and error handling
- Transaction submission and monitoring
- Rate limiting and security middleware
- 95%+ test coverage

## Quick Start

```bash
cd api
npm install
cp .env.example .env
# Edit .env with your configuration
npm run dev
```

## Configuration

Required environment variables in `.env`:

```env
PORT=3000
STELLAR_NETWORK=testnet
HORIZON_URL=https://horizon-testnet.stellar.org
SOROBAN_RPC_URL=https://soroban-testnet.stellar.org
CONTRACT_ID=<your_deployed_contract_id>
JWT_SECRET=<your_secret_key>
STELLAR_API_HOOK_SECRET=<your_hook_secret>
```

Circuit breaker tuning (optional, environment variables):

```env
# Rolling window size in ms used to compute failure rate (default 60000)
CB_WINDOW_MS=60000
# Failure rate (fraction) above which the circuit opens (default 0.5)
CB_FAILURE_THRESHOLD=0.5
# Minimum number of requests in window before evaluating failure rate (default 5)
CB_MIN_REQUESTS=5
# Time to keep circuit OPEN in ms before transitioning to HALF_OPEN (default 30000)
CB_OPEN_MS=30000
# Number of successful trial requests in HALF_OPEN to close circuit (default 2)
CB_HALF_OPEN_TRIAL=2
```

## API Endpoints

### Health Check
`GET /api/health` - Check service status

### Deep Health Check
- `GET /api/health/healthz` - Deep liveness and readiness check. Returns structured status and diagnostic info:

  - `rpc` (boolean): whether Soroban RPC responded to health probe
  - `contract` (boolean): whether the configured lending contract is reachable (invocation)
  - `ledger` (number|null): latest ledger sequence observed from Horizon when available

  Returns HTTP `200` when both `rpc` and `contract` are true, otherwise `503`.

SLO: The service aims for 99.9% availability for `/api/health/healthz` (RPC + contract reachable). Use a short scrape interval (e.g. 10s) and alert on consecutive failures for 1 minute.
The health endpoint now includes Soroban RPC circuit breaker metrics under `services.sorobanBreaker`:

```json
{
  "status": "healthy|unhealthy",
  "timestamp": "...",
  "services": {
    "horizon": true,
    "sorobanRpc": true,
    "sorobanBreaker": {
      "state": "CLOSED|OPEN|HALF_OPEN",
      "windowMs": 60000,
      "total": 10,
      "failures": 3,
      "failureRate": 0.3
    }
  }
}
```

### Deposit Collateral
`POST /api/lending/deposit`
```json
{
  "userAddress": "G...",
  "amount": "10000000",
  "userSecret": "S..."
}
```

### Borrow Assets
`POST /api/lending/borrow`
```json
{
  "userAddress": "G...",
  "amount": "5000000",
  "userSecret": "S..."
}
```

### Repay Debt
`POST /api/lending/repay`
```json
{
  "userAddress": "G...",
  "amount": "5500000",
  "userSecret": "S..."
}
```

### Withdraw Collateral
`POST /api/lending/withdraw`
```json
{
  "userAddress": "G...",
  "amount": "2000000",
  "userSecret": "S..."
}
```

All amounts in stroops (1 XLM = 10,000,000 stroops)

## Request Validation

Lending routes validate request bodies before calling Stellar or Soroban services:

- `userAddress` must be a valid Stellar account or contract address.
- `assetAddress` is optional, but when provided must be a valid Stellar account or contract address.
- `amount` must be a base-10 integer string, greater than zero, and within the signed i128 range (`-170141183460469231731687303715884105728` through `170141183460469231731687303715884105727`).
- `userSecret` is required and must be a non-empty string.

Validation failures return HTTP `400` with the shared error shape:

```json
{
  "success": false,
  "error": "userAddress: Must be a valid Stellar account or contract address"
}
```

## Testing

```bash
npm test              # Run all tests
npm test -- --coverage  # With coverage report
```

Test coverage: 95%+ (branches, functions, lines, statements)

## Indexer write hook authentication

The API now protects internal indexer write hooks using HMAC-SHA256 and a shared secret. Every request to `/api/lending/hooks/*` must include the following headers:

- `X-Hook-Timestamp`: the request timestamp in milliseconds since epoch
- `X-Hook-Signature`: hex-encoded HMAC-SHA256 over the string `timestamp + '.' + requestBody`

The middleware rejects requests when:

- the hook secret is not configured
- either header is missing or malformed
- the timestamp is outside a 5-minute window
- the signature does not match the computed HMAC

### Secret rotation

1. Generate a new strong secret and set it as `STELLAR_API_HOOK_SECRET` in the API deployment.
2. Update the hook sender(s) to sign requests with the new secret.
3. Deploy the sender and API together or with an overlap window.
4. Validate that hook requests are accepted and monitor for authentication failures.
5. Remove the previous secret from all systems once the new secret is active.

## Production Build

```bash
npm run build
npm start
```

## Project Structure

```
api/src/
├── __tests__/      # Test files
├── config/         # Configuration
├── controllers/    # Request handlers
├── middleware/     # Validation, auth, errors
├── routes/         # API routes
├── services/       # Stellar integration
├── types/          # TypeScript types
└── utils/          # Logger, errors
```

## License

MIT
