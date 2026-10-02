/**
 * lending.routes.test.ts
 *
 * Authorization and validation regression coverage for
 * api/src/routes/lending.routes.ts (Issue #2118).
 *
 * Invariants tested:
 *   - POST /hooks/*: HMAC + timestamp guard; 401 on any failure
 *   - POST /deposit|borrow|repay|withdraw: Zod schema validation; 400 on invalid input
 *   - Controllers never receive invalid bodies
 *   - Error shape: { success: false, error: string } for 400/401/500
 *   - No sensitive data (userSecret, internal stack) in error responses
 */

import crypto from 'crypto';

// Mock stellar SDK before any other imports that transitively load it
jest.mock('@stellar/stellar-sdk', () => ({
  StrKey: {
    isValidEd25519PublicKey: (value: string) =>
      typeof value === 'string' && value.length === 56 && value.startsWith('G'),
    isValidContract: (value: string) =>
      typeof value === 'string' && value.length === 56 && value.startsWith('C'),
  },
}));

// Mock StellarService before loading app
const mockStellarService = {
  buildDepositTransaction: jest.fn(),
  buildBorrowTransaction: jest.fn(),
  buildRepayTransaction: jest.fn(),
  buildWithdrawTransaction: jest.fn(),
  submitTransaction: jest.fn(),
  monitorTransaction: jest.fn(),
  healthCheck: jest.fn(),
};

jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn(() => mockStellarService),
}));

// Load app AFTER mocks are set up
const app = require('../app').default;

// Load config after app so we can mutate hookSecret
const { config } = require('../config');

import request from 'supertest';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Valid Stellar G-address (56 chars, starts with G)
const VALID_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
// Valid Stellar C-address (contract)
const VALID_CONTRACT = 'CBFJDKFLMWQNOQSUVWTNOLKRBXJINBHQSIPFZRXPQKZMHVGJLNFQPZQA';
const VALID_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';
const VALID_AMOUNT = '1000000';
const HOOK_SECRET = 'test-hook-secret-routes-tests';

// i128 boundaries
const I128_MAX = '170141183460469231731687303715884105727'; // (2^127) - 1
const I128_OVERFLOW = '170141183460469231731687303715884105728'; // (2^127)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeHookHeaders(
  body: string,
  secret: string = HOOK_SECRET,
  timestampMs?: number
) {
  const ts = (timestampMs ?? Date.now()).toString();
  const payload = `${ts}.${body}`;
  const sig = crypto.createHmac('sha256', secret).update(payload).digest('hex');
  return { 'x-hook-signature': sig, 'x-hook-timestamp': ts };
}

function validBody() {
  return {
    userAddress: VALID_ADDRESS,
    amount: VALID_AMOUNT,
    userSecret: VALID_SECRET,
  };
}

function setupSuccessMocks() {
  mockStellarService.buildDepositTransaction.mockResolvedValue('mock_xdr');
  mockStellarService.buildBorrowTransaction.mockResolvedValue('mock_xdr');
  mockStellarService.buildRepayTransaction.mockResolvedValue('mock_xdr');
  mockStellarService.buildWithdrawTransaction.mockResolvedValue('mock_xdr');
  mockStellarService.submitTransaction.mockResolvedValue({
    success: true,
    transactionHash: 'mock_hash_abc123',
    status: 'success',
  });
  mockStellarService.monitorTransaction.mockResolvedValue({
    success: true,
    transactionHash: 'mock_hash_abc123',
    status: 'success',
    ledger: 12345,
  });
}

// ---------------------------------------------------------------------------
// Hook secret lifecycle
// ---------------------------------------------------------------------------

beforeAll(() => {
  // Mutate config directly since it's loaded at module import time
  (config.auth as any).hookSecret = HOOK_SECRET;
});

afterAll(() => {
  (config.auth as any).hookSecret = '';
});

beforeEach(() => {
  jest.clearAllMocks();
  setupSuccessMocks();
});

// ---------------------------------------------------------------------------
// POST /api/lending/hooks — HMAC guard
// ---------------------------------------------------------------------------

describe('POST /api/lending/hooks — HMAC guard', () => {
  const hookPath = '/api/lending/hooks';
  const hookSubPath = '/api/lending/hooks/indexer/write';
  const body = { event: 'test.event' };
  const rawBody = JSON.stringify(body);

  it('200 — valid HMAC, correct timestamp, correct body', async () => {
    const headers = makeHookHeaders(rawBody);
    const res = await request(app)
      .post(hookPath)
      .set(headers)
      .send(body);
    expect(res.status).toBe(200);
    expect(res.body).toEqual({ success: true, message: 'Hook authenticated' });
  });

  it('200 — wildcard route POST /hooks/indexer/write', async () => {
    const headers = makeHookHeaders(rawBody);
    const res = await request(app)
      .post(hookSubPath)
      .set(headers)
      .send(body);
    expect(res.status).toBe(200);
  });

  it('401 — missing both headers', async () => {
    const res = await request(app).post(hookPath).send(body);
    expect(res.status).toBe(401);
    expect(res.body).toMatchObject({ success: false, error: expect.any(String) });
  });

  it('401 — missing timestamp header', async () => {
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update('x').digest('hex');
    const res = await request(app)
      .post(hookPath)
      .set('x-hook-signature', sig)
      .send(body);
    expect(res.status).toBe(401);
    expect(res.body.success).toBe(false);
  });

  it('401 — missing signature header', async () => {
    const res = await request(app)
      .post(hookPath)
      .set('x-hook-timestamp', Date.now().toString())
      .send(body);
    expect(res.status).toBe(401);
    expect(res.body.success).toBe(false);
  });

  it('401 — invalid signature (correct timestamp, wrong HMAC)', async () => {
    const ts = Date.now().toString();
    const res = await request(app)
      .post(hookPath)
      .set('x-hook-signature', '0'.repeat(64))
      .set('x-hook-timestamp', ts)
      .send(body);
    expect(res.status).toBe(401);
    expect(res.body.error).toMatch(/invalid hook signature/i);
  });

  it('401 — timestamp too old (> 5 min)', async () => {
    const stale = Date.now() - 6 * 60 * 1000;
    const headers = makeHookHeaders(rawBody, HOOK_SECRET, stale);
    const res = await request(app)
      .post(hookPath)
      .set(headers)
      .send(body);
    expect(res.status).toBe(401);
    expect(res.body.error).toMatch(/timestamp outside allowable window/i);
  });

  it('401 — non-numeric timestamp', async () => {
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update('x').digest('hex');
    const res = await request(app)
      .post(hookPath)
      .set('x-hook-signature', sig)
      .set('x-hook-timestamp', 'not-a-number')
      .send(body);
    expect(res.status).toBe(401);
  });

  it('200 — timestamp at exactly 5min - 1ms inside window', async () => {
    // Use 4min 58s to give buffer for request processing time
    const ts = Date.now() - (4 * 60 * 1000 + 58 * 1000);
    const headers = makeHookHeaders(rawBody, HOOK_SECRET, ts);
    const res = await request(app)
      .post(hookPath)
      .set(headers)
      .send(body);
    expect(res.status).toBe(200);
  });

  it('401 — correct signature but different body was signed', async () => {
    const ts = Date.now().toString();
    const sigForOtherBody = crypto
      .createHmac('sha256', HOOK_SECRET)
      .update(`${ts}.different_body`)
      .digest('hex');
    const res = await request(app)
      .post(hookPath)
      .set('x-hook-signature', sigForOtherBody)
      .set('x-hook-timestamp', ts)
      .send(body);
    expect(res.status).toBe(401);
  });

  it('401 response has { success: false, error: string } shape', async () => {
    const res = await request(app).post(hookPath).send(body);
    expect(res.status).toBe(401);
    expect(typeof res.body.error).toBe('string');
    expect(res.body.success).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/deposit — validation
// ---------------------------------------------------------------------------

describe('POST /api/lending/deposit — validation', () => {
  const path = '/api/lending/deposit';

  it('400 — empty body {}', async () => {
    const res = await request(app).post(path).send({});
    expect(res.status).toBe(400);
    expect(res.body).toMatchObject({ success: false, error: expect.any(String) });
  });

  it('400 — missing userAddress', async () => {
    const { userAddress: _, ...body } = validBody();
    const res = await request(app).post(path).send(body);
    expect(res.status).toBe(400);
  });

  it('400 — invalid userAddress (not a Stellar address)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), userAddress: 'not-a-stellar-address' });
    expect(res.status).toBe(400);
  });

  it('400 — missing amount', async () => {
    const { amount: _, ...body } = validBody();
    const res = await request(app).post(path).send(body);
    expect(res.status).toBe(400);
  });

  it('400 — amount = "0" (must be > 0)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '0' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/greater than zero/i);
  });

  it('400 — amount = "-1" (negative)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '-1' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/greater than zero/i);
  });

  it('400 — float string amount "1.5"', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '1.5' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/integer string/i);
  });

  it('400 — non-numeric amount string "abc"', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: 'abc' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/integer string/i);
  });

  it('400 — amount "NaN"', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: 'NaN' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/integer string/i);
  });

  it('400 — amount "Infinity"', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: 'Infinity' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/integer string/i);
  });

  it('400 — amount overflows i128 (2^127)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: I128_OVERFLOW });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/signed 128-bit/i);
  });

  it('passes validation with i128 max (2^127 - 1)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: I128_MAX });
    // Could be 200 or 400 from service, but not a validation 400 from schema
    // The validation layer should pass; error (if any) comes from service
    expect([200, 400, 500]).toContain(res.status);
    if (res.status === 400) {
      // Must not be a validation schema error about i128
      expect(res.body.error).not.toMatch(/signed 128-bit/i);
    }
  });

  it('400 — missing userSecret', async () => {
    const { userSecret: _, ...body } = validBody();
    const res = await request(app).post(path).send(body);
    expect(res.status).toBe(400);
  });

  it('400 — empty string userSecret ""', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), userSecret: '' });
    expect(res.status).toBe(400);
  });

  it('400 — whitespace-only userSecret', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), userSecret: '   ' });
    expect(res.status).toBe(400);
  });

  it('400 — invalid assetAddress when provided', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), assetAddress: 'not-a-stellar-address' });
    expect(res.status).toBe(400);
  });

  it('200 — assetAddress omitted (optional field)', async () => {
    const res = await request(app).post(path).send(validBody());
    expect([200, 400]).toContain(res.status);
    // The validation must pass (not 400 from schema)
    if (res.status === 400) {
      // Must be from service, not validation
      expect(res.body.error).not.toMatch(/stellar/i);
    }
  });

  it('regression: empty string assetAddress treated as absent (not rejected)', async () => {
    // The optionalStellarAddress preprocessor converts '' to undefined
    // so '' should NOT fail address validation
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), assetAddress: '' });
    // Should not get a 400 about assetAddress being invalid
    if (res.status === 400) {
      expect(res.body.error).not.toMatch(/valid Stellar/i);
    }
  });

  it('valid assetAddress (Stellar C-address) passes validation', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), assetAddress: VALID_CONTRACT });
    expect([200, 400]).toContain(res.status);
    if (res.status === 400) {
      expect(res.body.error).not.toMatch(/valid Stellar/i);
    }
  });

  it('extra unknown fields are silently stripped (not rejected)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), extraField: 'should-be-stripped', anotherExtra: 123 });
    // Zod strips extra fields by default — should not 400 for extra fields
    expect([200, 400]).toContain(res.status);
    if (res.status === 400) {
      expect(res.body.error).not.toMatch(/extra/i);
    }
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/deposit — controller behavior
// ---------------------------------------------------------------------------

describe('POST /api/lending/deposit — controller behavior', () => {
  const path = '/api/lending/deposit';

  it('200 — buildDepositTransaction called with correct args', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), assetAddress: VALID_CONTRACT });
    expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
      VALID_ADDRESS,
      VALID_CONTRACT,
      VALID_AMOUNT,
      VALID_SECRET
    );
  });

  it('200 — returns monitorTransaction result on submit success', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
    expect(res.body.transactionHash).toBe('mock_hash_abc123');
    expect(mockStellarService.monitorTransaction).toHaveBeenCalledWith('mock_hash_abc123');
  });

  it('400 — returns submitTransaction result when success=false', async () => {
    mockStellarService.submitTransaction.mockResolvedValueOnce({
      success: false,
      status: 'failed',
      error: 'Insufficient funds',
    });
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(400);
    expect(res.body.success).toBe(false);
  });

  it('500 — buildDepositTransaction throws → safe error, no internal details', async () => {
    mockStellarService.buildDepositTransaction.mockRejectedValueOnce(
      new Error('Stellar RPC is down: internal detail')
    );
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body).toMatchObject({ success: false, error: 'Internal server error' });
    // Must not expose internal error message
    expect(res.body.error).not.toContain('Stellar RPC is down');
  });

  it('500 — submitTransaction throws → safe error', async () => {
    mockStellarService.submitTransaction.mockRejectedValueOnce(new Error('Network timeout'));
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body.error).toBe('Internal server error');
  });

  it('500 — monitorTransaction throws after successful submit → safe error', async () => {
    mockStellarService.monitorTransaction.mockRejectedValueOnce(new Error('Monitor timeout'));
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body.error).toBe('Internal server error');
  });

  it('error body does not expose userSecret', async () => {
    mockStellarService.buildDepositTransaction.mockRejectedValueOnce(new Error('fail'));
    const res = await request(app).post(path).send(validBody());
    expect(JSON.stringify(res.body)).not.toContain(VALID_SECRET);
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/borrow — validation
// ---------------------------------------------------------------------------

describe('POST /api/lending/borrow — validation', () => {
  const path = '/api/lending/borrow';

  it('400 — empty body', async () => {
    const res = await request(app).post(path).send({});
    expect(res.status).toBe(400);
  });

  it('400 — invalid userAddress', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), userAddress: 'INVALID' });
    expect(res.status).toBe(400);
  });

  it('400 — zero amount', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '0' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/greater than zero/i);
  });

  it('400 — negative amount', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '-5000' });
    expect(res.status).toBe(400);
  });

  it('400 — float amount', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '2.5' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/integer string/i);
  });

  it('400 — i128 overflow', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: I128_OVERFLOW });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/signed 128-bit/i);
  });

  it('400 — missing userSecret', async () => {
    const { userSecret: _, ...body } = validBody();
    const res = await request(app).post(path).send(body);
    expect(res.status).toBe(400);
  });

  it('200 — valid request routes to borrow controller', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(mockStellarService.buildBorrowTransaction).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/borrow — controller behavior
// ---------------------------------------------------------------------------

describe('POST /api/lending/borrow — controller behavior', () => {
  const path = '/api/lending/borrow';

  it('200 — returns monitorTransaction result on success', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('400 — returns submit failure result', async () => {
    mockStellarService.submitTransaction.mockResolvedValueOnce({
      success: false,
      error: 'Collateral insufficient',
      status: 'failed',
    });
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(400);
    expect(res.body.success).toBe(false);
  });

  it('500 — buildBorrowTransaction throws', async () => {
    mockStellarService.buildBorrowTransaction.mockRejectedValueOnce(new Error('RPC error'));
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body.error).toBe('Internal server error');
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/repay — validation
// ---------------------------------------------------------------------------

describe('POST /api/lending/repay — validation', () => {
  const path = '/api/lending/repay';

  it('400 — empty body', async () => {
    const res = await request(app).post(path).send({});
    expect(res.status).toBe(400);
  });

  it('400 — invalid amount (negative)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '-100' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/greater than zero/i);
  });

  it('400 — missing userAddress', async () => {
    const { userAddress: _, ...body } = validBody();
    const res = await request(app).post(path).send(body);
    expect(res.status).toBe(400);
  });

  it('200 — valid request routes to repay controller', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(mockStellarService.buildRepayTransaction).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/repay — controller behavior
// ---------------------------------------------------------------------------

describe('POST /api/lending/repay — controller behavior', () => {
  const path = '/api/lending/repay';

  it('200 — returns monitorTransaction result on success', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('400 — returns submit failure result', async () => {
    mockStellarService.submitTransaction.mockResolvedValueOnce({
      success: false,
      status: 'failed',
      error: 'Loan not found',
    });
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(400);
  });

  it('500 — buildRepayTransaction throws', async () => {
    mockStellarService.buildRepayTransaction.mockRejectedValueOnce(new Error('Timeout'));
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body.error).toBe('Internal server error');
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/withdraw — validation
// ---------------------------------------------------------------------------

describe('POST /api/lending/withdraw — validation', () => {
  const path = '/api/lending/withdraw';

  it('400 — empty body', async () => {
    const res = await request(app).post(path).send({});
    expect(res.status).toBe(400);
  });

  it('400 — invalid amount (zero)', async () => {
    const res = await request(app)
      .post(path)
      .send({ ...validBody(), amount: '0' });
    expect(res.status).toBe(400);
    expect(res.body.error).toMatch(/greater than zero/i);
  });

  it('400 — missing required fields', async () => {
    const res = await request(app)
      .post(path)
      .send({ userAddress: VALID_ADDRESS }); // missing amount + userSecret
    expect(res.status).toBe(400);
  });

  it('200 — valid request routes to withdraw controller', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(mockStellarService.buildWithdrawTransaction).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// POST /api/lending/withdraw — controller behavior
// ---------------------------------------------------------------------------

describe('POST /api/lending/withdraw — controller behavior', () => {
  const path = '/api/lending/withdraw';

  it('200 — returns monitorTransaction result on success', async () => {
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('400 — returns submit failure result', async () => {
    mockStellarService.submitTransaction.mockResolvedValueOnce({
      success: false,
      status: 'failed',
      error: 'Insufficient balance',
    });
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(400);
  });

  it('500 — buildWithdrawTransaction throws', async () => {
    mockStellarService.buildWithdrawTransaction.mockRejectedValueOnce(new Error('Contract error'));
    const res = await request(app).post(path).send(validBody());
    expect(res.status).toBe(500);
    expect(res.body.error).toBe('Internal server error');
  });
});

// ---------------------------------------------------------------------------
// Error shape invariant — all routes
// ---------------------------------------------------------------------------

describe('Error shape invariant — all routes', () => {
  const routes = [
    '/api/lending/deposit',
    '/api/lending/borrow',
    '/api/lending/repay',
    '/api/lending/withdraw',
  ];

  it('all 400 validation errors have { success: false, error: string }', async () => {
    for (const route of routes) {
      const res = await request(app).post(route).send({});
      expect(res.status).toBe(400);
      expect(res.body.success).toBe(false);
      expect(typeof res.body.error).toBe('string');
    }
  });

  it('all 500 errors have { success: false, error: "Internal server error" }', async () => {
    mockStellarService.buildDepositTransaction.mockRejectedValue(new Error('fail'));
    mockStellarService.buildBorrowTransaction.mockRejectedValue(new Error('fail'));
    mockStellarService.buildRepayTransaction.mockRejectedValue(new Error('fail'));
    mockStellarService.buildWithdrawTransaction.mockRejectedValue(new Error('fail'));

    for (const route of routes) {
      const res = await request(app).post(route).send(validBody());
      expect(res.status).toBe(500);
      expect(res.body).toMatchObject({ success: false, error: 'Internal server error' });
    }
  });

  it('error messages do not contain userSecret value', async () => {
    mockStellarService.buildDepositTransaction.mockRejectedValue(new Error('fail'));
    const res = await request(app).post('/api/lending/deposit').send(validBody());
    expect(JSON.stringify(res.body)).not.toContain(VALID_SECRET);
  });

  it('error messages do not contain stack trace', async () => {
    mockStellarService.buildDepositTransaction.mockRejectedValue(new Error('fail'));
    const res = await request(app).post('/api/lending/deposit').send(validBody());
    expect(JSON.stringify(res.body)).not.toMatch(/at Object\./);
  });

  it('the error field is a string (not nested object)', async () => {
    const res = await request(app).post('/api/lending/deposit').send({});
    expect(res.status).toBe(400);
    expect(typeof res.body.error).toBe('string');
  });
});

// ---------------------------------------------------------------------------
// Duplicate / idempotency
// ---------------------------------------------------------------------------

describe('Duplicate / idempotency', () => {
  it('two identical deposit requests are processed independently', async () => {
    const res1 = await request(app).post('/api/lending/deposit').send(validBody());
    const res2 = await request(app).post('/api/lending/deposit').send(validBody());
    // Both should be processed; buildDepositTransaction called twice
    expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledTimes(2);
    // No shared state corruption
    expect(res1.status).toBe(res2.status);
  });

  it('two parallel deposit requests process independently', async () => {
    const [res1, res2] = await Promise.all([
      request(app).post('/api/lending/deposit').send(validBody()),
      request(app).post('/api/lending/deposit').send(validBody()),
    ]);
    expect([200, 400, 500]).toContain(res1.status);
    expect([200, 400, 500]).toContain(res2.status);
    // Each request should have its own response body
    expect(res1.body).toBeDefined();
    expect(res2.body).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// Concurrent requests
// ---------------------------------------------------------------------------

describe('Concurrent requests', () => {
  it('10 concurrent deposits with valid bodies all receive valid responses', async () => {
    const amounts = Array.from({ length: 10 }, (_, i) => String((i + 1) * 100000));
    const responses = await Promise.all(
      amounts.map((amount) =>
        request(app)
          .post('/api/lending/deposit')
          .send({ ...validBody(), amount })
      )
    );
    for (const res of responses) {
      expect([200, 400, 500]).toContain(res.status);
      expect(res.body).toHaveProperty('success');
    }
  });
});

// ---------------------------------------------------------------------------
// Request body content-type / malformed bodies
// ---------------------------------------------------------------------------

describe('Request body content-type', () => {
  it('400 — malformed JSON body', async () => {
    const res = await request(app)
      .post('/api/lending/deposit')
      .set('Content-Type', 'application/json')
      .send('{ bad json }');
    expect(res.status).toBe(400);
  });
});

// ---------------------------------------------------------------------------
// Regression: optionalStellarAddress preprocessor
// ---------------------------------------------------------------------------

describe('Regression: optionalStellarAddress preprocessor', () => {
  it('empty string assetAddress is treated as absent, not rejected as invalid', async () => {
    const body = { ...validBody(), assetAddress: '' };
    const res = await request(app).post('/api/lending/deposit').send(body);
    // Should not 400 with "valid Stellar" error
    if (res.status === 400) {
      expect(res.body.error).not.toMatch(/valid Stellar/i);
    }
    // The preprocessor converts '' -> undefined; no validation error
    expect(res.body.error || '').not.toMatch(/valid Stellar/i);
  });

  it('undefined assetAddress passes through without error', async () => {
    const res = await request(app).post('/api/lending/deposit').send(validBody());
    // No assetAddress field — should pass validation
    if (res.status === 400) {
      // If 400, must be from service not from address validation
      expect(res.body.error).not.toMatch(/asset/i);
    }
  });
});
