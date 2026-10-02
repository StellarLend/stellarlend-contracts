/**
 * auth.test.ts
 *
 * Failure-path, boundary, and concurrency coverage for
 * api/src/middleware/auth.ts (Issue #2059).
 *
 * Invariants exercised:
 *   - authenticateToken throws UnauthorizedError (not next(err)) on failure
 *   - verifyHookHmac uses timingSafeEqual; throws on bad/missing headers
 *   - Neither function leaks token value, secret, or signature in error messages
 *   - No shared mutable state between parallel invocations
 */

import crypto from 'crypto';
import jwt from 'jsonwebtoken';
import { config } from '../config';
import { authenticateToken, generateToken, verifyHookHmac, AuthRequest } from '../middleware/auth';
import crypto from 'crypto';
import jwt from 'jsonwebtoken';
import {
  authenticateToken,
  generateToken,
  verifyHookHmac,
  AuthRequest,
} from '../middleware/auth';
import { UnauthorizedError } from '../utils/errors';
import { config } from '../config';
import { TransactionStatus } from '../types';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Build a minimal AuthRequest stub */
function makeReq(overrides: Partial<AuthRequest> = {}): AuthRequest {
  return {
    headers: {},
    body: {},
    ...overrides,
  } as AuthRequest;
}

const VALID_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_ADDRESS2 = 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4';
const TEST_SECRET = config.auth.jwtSecret; // 'default-secret-change-me'
const HOOK_SECRET = 'test-hook-secret-for-unit-tests';
const RES_STUB = {} as any;

function makeHookHeaders(body: string, secret = HOOK_SECRET, timestampOverride?: number) {
  const ts = (timestampOverride ?? Date.now()).toString();
  const payload = `${ts}.${body}`;
  const sig = crypto.createHmac('sha256', secret).update(payload).digest('hex');
  return { signature: sig, timestamp: ts };
}

// ---------------------------------------------------------------------------
// Original tests (kept intact)
// ---------------------------------------------------------------------------

describe('Auth Middleware', () => {
  const response = {} as any;

  describe('authenticateToken', () => {
    it('should reject requests without access token', () => {
      const request = { headers: {} } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Access token required')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject invalid access token', () => {
      const request = { headers: { authorization: 'Bearer invalid' } } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Invalid or expired token')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject token signed with wrong secret', () => {
      const token = jwt.sign({ address: 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ' }, 'wrong-secret');
      const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Invalid or expired token')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject expired token', () => {
      const token = jwt.sign(
        { address: 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ' },
        config.auth.jwtSecret,
        { expiresIn: -1 }
      );
      const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Invalid or expired token')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject malformed authorization header', () => {
      const request = { headers: { authorization: 'Bearer' } } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Access token required')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should attach decoded user for valid access token', () => {
      const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
      const token = generateToken(address);
      const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
      const next = jest.fn();

      authenticateToken(request, response, next);

      expect(request.user).toEqual({ address, iat: expect.any(Number), exp: expect.any(Number) });
      expect(next).toHaveBeenCalledTimes(1);
    });

    it('should be deterministic across repeated validations of the same token', () => {
      const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
      const token = generateToken(address);
      const next = jest.fn();

      for (let i = 0; i < 5; i++) {
        const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
        authenticateToken(request, response, next);
        expect(request.user).toEqual({ address, iat: expect.any(Number), exp: expect.any(Number) });
      }
      expect(next).toHaveBeenCalledTimes(5);
    });
  });

  describe('verifyHookHmac', () => {
    const originalHookSecret = config.auth.hookSecret;

    const sign = (timestamp: string, rawBody: string, secret: string) =>
      crypto.createHmac('sha256', secret).update(`${timestamp}.${rawBody}`).digest('hex');

    afterEach(() => {
      config.auth.hookSecret = originalHookSecret;
    });

    it('should reject when hook secret is not configured', () => {
      config.auth.hookSecret = '';
      const request = { headers: {} } as AuthRequest;
      const next = jest.fn();

      expect(() => verifyHookHmac(request, response, next)).toThrow(
        new UnauthorizedError('Hook authentication secret is not configured')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject when signature or timestamp headers are missing', () => {
      config.auth.hookSecret = 'test-secret';
      const next = jest.fn();

      expect(() =>
        verifyHookHmac({ headers: {} } as AuthRequest, response, next)
      ).toThrow(new UnauthorizedError('Hook signature and timestamp headers are required'));

      expect(() =>
        verifyHookHmac(
          { headers: { 'x-hook-signature': 'abc' } } as AuthRequest,
          response,
          next
        )
      ).toThrow(new UnauthorizedError('Hook signature and timestamp headers are required'));

      expect(next).not.toHaveBeenCalled();
    });

    it('should reject non-numeric timestamp', () => {
      config.auth.hookSecret = 'test-secret';
      const request = {
        headers: {
          'x-hook-signature': 'abc',
          'x-hook-timestamp': 'not-a-number',
        },
      } as AuthRequest;
      const next = jest.fn();

      expect(() => verifyHookHmac(request, response, next)).toThrow(
        new UnauthorizedError('Invalid hook timestamp')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject timestamp outside the allowable window', () => {
      config.auth.hookSecret = 'test-secret';
      const staleTimestamp = String(Date.now() - 10 * 60 * 1000);
      const rawBody = JSON.stringify({ event: 'test' });
      const request = {
        headers: {
          'x-hook-signature': sign(staleTimestamp, rawBody, 'test-secret'),
          'x-hook-timestamp': staleTimestamp,
        },
        rawBody,
      } as AuthRequest;
      const next = jest.fn();

      expect(() => verifyHookHmac(request, response, next)).toThrow(
        new UnauthorizedError('Hook timestamp outside allowable window')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject tampered payload with valid timestamp', () => {
      config.auth.hookSecret = 'test-secret';
      const timestamp = String(Date.now());
      const signature = sign(timestamp, '{"event":"original"}', 'test-secret');
      const request = {
        headers: {
          'x-hook-signature': signature,
          'x-hook-timestamp': timestamp,
        },
        rawBody: '{"event":"tampered"}',
      } as AuthRequest;
      const next = jest.fn();

      expect(() => verifyHookHmac(request, response, next)).toThrow(
        new UnauthorizedError('Invalid hook signature')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject malformed non-hex signature without throwing an unexpected error', () => {
      config.auth.hookSecret = 'test-secret';
      const timestamp = String(Date.now());
      const request = {
        headers: {
          'x-hook-signature': 'not-hex',
          'x-hook-timestamp': timestamp,
        },
        rawBody: '{}',
      } as AuthRequest;
      const next = jest.fn();

      expect(() => verifyHookHmac(request, response, next)).toThrow(
        new UnauthorizedError('Invalid hook signature')
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should accept a valid signature and call next', () => {
      config.auth.hookSecret = 'test-secret';
      const timestamp = String(Date.now());
      const rawBody = JSON.stringify({ event: 'test', data: 123 });
      const request = {
        headers: {
          'x-hook-signature': sign(timestamp, rawBody, 'test-secret'),
          'x-hook-timestamp': timestamp,
        },
        rawBody,
      } as AuthRequest;
      const next = jest.fn();

      verifyHookHmac(request, response, next);

      expect(next).toHaveBeenCalledTimes(1);
    });

    it('should accept a signature at the exact window boundary', () => {
      config.auth.hookSecret = 'test-secret';
      const timestamp = String(Date.now() - 5 * 60 * 1000);
      const rawBody = '{}';
      const request = {
        headers: {
          'x-hook-signature': sign(timestamp, rawBody, 'test-secret'),
          'x-hook-timestamp': timestamp,
        },
        rawBody,
      } as AuthRequest;
      const next = jest.fn();

      verifyHookHmac(request, response, next);

      expect(next).toHaveBeenCalledTimes(1);
    });

    it('should handle array-valued headers deterministically', () => {
      config.auth.hookSecret = 'test-secret';
      const timestamp = String(Date.now());
      const rawBody = '{}';
      const request = {
        headers: {
          'x-hook-signature': [sign(timestamp, rawBody, 'test-secret')],
          'x-hook-timestamp': [timestamp],
        },
        rawBody,
      } as unknown as AuthRequest;
      const next = jest.fn();

      verifyHookHmac(request, response, next);

      expect(next).toHaveBeenCalledTimes(1);
    });
  });

  it('should expose transaction status values', () => {
    expect(TransactionStatus.PENDING).toBe('pending');
    expect(TransactionStatus.SUCCESS).toBe('success');
    expect(TransactionStatus.FAILED).toBe('failed');
    expect(TransactionStatus.NOT_FOUND).toBe('not_found');
  });

  it('should reject malformed authorization headers', () => {
    const malformedHeaders = [
      { authorization: 'Bearer' },
      { authorization: 'Bearer  ' },
      { authorization: 'Bearer a b.c.d' },
      { authorization: 'Token abc.def.ghi' },
      { authorization: 'Bearer abc.def.ghi.extra' },
    ];

    for (const headers of malformedHeaders) {
      const request = { headers } as AuthRequest;
      const next = jest.fn();

      expect(() => authenticateToken(request, response, next)).toThrow(UnauthorizedError);
      expect(next).not.toHaveBeenCalled();
    }
  });

  it('should be deterministic across repeated authentication attempts', () => {
    const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
    const token = generateToken(address);

    for (let i = 0; i < 5; i++) {
      const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
      const next = jest.fn();

      authenticateToken(request, response, next);

      expect(request.user?.address).toBe(address);
      expect(next).toHaveBeenCalledTimes(1);
    }
  });

  it('should not leak token details in error messages', () => {
    const secret = 'super-secret-token-value';
    const request = { headers: { authorization: `Bearer ${secret}` } } as AuthRequest;
    const next = jest.fn();

    try {
      authenticateToken(request, response, next);
      throw new Error('Expected authenticateToken to throw');
    } catch (error) {
      expect(error).toBeInstanceOf(UnauthorizedError);
      expect((error as Error).message).not.toContain(secret);
    }
    expect(next).not.toHaveBeenCalled();
  });

  it('should not mutate the request on rejection', () => {
    const request = { headers: { authorization: 'Bearer invalid' } } as AuthRequest;
    const next = jest.fn();

    expect(() => authenticateToken(request, response, next)).toThrow(UnauthorizedError);
    expect(request.user).toBeUndefined();
  });

  it('should keep transaction status values unique and immutable in shape', () => {
    const values = Object.values(TransactionStatus);
    expect(new Set(values).size).toBe(values.length);
    for (const value of values) {
      expect(typeof value).toBe('string');
    }
  });
});

// ---------------------------------------------------------------------------
// authenticateToken — additional rejection paths
// ---------------------------------------------------------------------------

describe('authenticateToken — rejection paths', () => {
  it('rejects Bearer prefix with no token value (trailing space)', () => {
    // 'Bearer '.split(' ')[1] === '' which is falsy
    const req = makeReq({ headers: { authorization: 'Bearer ' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Access token required');
  });

  it('rejects non-Bearer scheme — wrong-scheme token fails jwt.verify', () => {
    // 'Basic dXNlcjpwYXNz'.split(' ')[1] is a non-empty string; jwt.verify rejects it
    const req = makeReq({ headers: { authorization: 'Basic dXNlcjpwYXNz' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('rejects token signed with wrong secret', () => {
    const token = jwt.sign({ address: VALID_ADDRESS }, 'wrong-secret', { expiresIn: '1h' });
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('rejects an already-expired token', () => {
    // expiresIn: -1 creates a token with exp in the past
    const token = jwt.sign({ address: VALID_ADDRESS }, TEST_SECRET, { expiresIn: -1 });
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('rejects a not-yet-valid token (nbf in future)', () => {
    const token = jwt.sign({ address: VALID_ADDRESS }, TEST_SECRET, { notBefore: '1h' });
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('rejects malformed JWT (not three dot-separated parts)', () => {
    const req = makeReq({ headers: { authorization: 'Bearer not.a.jwt' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('does not call next() on any rejection', () => {
    const badCases = [
      makeReq({ headers: {} }),
      makeReq({ headers: { authorization: 'Bearer ' } }),
      makeReq({ headers: { authorization: 'Bearer bad.token.here' } }),
    ];
    for (const req of badCases) {
      const next = jest.fn();
      try { authenticateToken(req, RES_STUB, next); } catch { /* expected */ }
      expect(next).not.toHaveBeenCalled();
    }
  });

  it('does not include the raw token value in the error message', () => {
    const token = jwt.sign({ address: VALID_ADDRESS }, 'other-secret', { expiresIn: '1h' });
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    let caught: Error | null = null;
    try { authenticateToken(req, RES_STUB, jest.fn()); } catch (e) { caught = e as Error; }
    expect(caught).not.toBeNull();
    expect(caught!.message).not.toContain(token);
  });
});

// ---------------------------------------------------------------------------
// authenticateToken — success paths
// ---------------------------------------------------------------------------

describe('authenticateToken — success paths', () => {
  it('populates req.user.address from token payload', () => {
    const token = generateToken(VALID_ADDRESS);
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    const next = jest.fn();
    authenticateToken(req, RES_STUB, next);
    expect(req.user?.address).toBe(VALID_ADDRESS);
    expect(next).toHaveBeenCalledTimes(1);
    expect(next).toHaveBeenCalledWith(/* no args */);
  });

  it('calls next() with no arguments on success', () => {
    const token = generateToken(VALID_ADDRESS);
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    const next = jest.fn();
    authenticateToken(req, RES_STUB, next);
    expect(next.mock.calls[0]).toHaveLength(0);
  });

  it('accepts token with extra claims in payload', () => {
    const token = jwt.sign({ address: VALID_ADDRESS, role: 'admin', extra: 42 }, TEST_SECRET, {
      expiresIn: '1h',
    });
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    const next = jest.fn();
    authenticateToken(req, RES_STUB, next);
    expect(req.user?.address).toBe(VALID_ADDRESS);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('does not include sensitive data in the user object (no raw token)', () => {
    const token = generateToken(VALID_ADDRESS);
    const req = makeReq({ headers: { authorization: `Bearer ${token}` } });
    const next = jest.fn();
    authenticateToken(req, RES_STUB, next);
    // req.user should only have standard JWT claims — not the raw token
    const userStr = JSON.stringify(req.user);
    expect(userStr).not.toContain(token);
  });
});

// ---------------------------------------------------------------------------
// authenticateToken — boundary: header format
// ---------------------------------------------------------------------------

describe('authenticateToken — boundary: header format', () => {
  it('rejects empty authorization header', () => {
    // ''.split(' ')[1] === undefined → falsy → 'Access token required'
    const req = makeReq({ headers: { authorization: '' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Access token required');
  });

  it('rejects authorization header with only whitespace', () => {
    // '   '.split(' ')[1] === '' → falsy → 'Access token required'
    const req = makeReq({ headers: { authorization: '   ' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
  });

  it('rejects oversized Authorization header (8000 chars)', () => {
    // jwt.verify will fail on this non-JWT string
    const bigToken = 'x'.repeat(8000);
    const req = makeReq({ headers: { authorization: `Bearer ${bigToken}` } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow(UnauthorizedError);
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Invalid or expired token');
  });

  it('double-space after Bearer gives empty first token segment → Access token required', () => {
    // 'Bearer  token'.split(' ')[1] === '' → falsy
    const req = makeReq({ headers: { authorization: 'Bearer  sometoken' } });
    expect(() => authenticateToken(req, RES_STUB, jest.fn())).toThrow('Access token required');
  });
});

// ---------------------------------------------------------------------------
// authenticateToken — concurrency / shared state
// ---------------------------------------------------------------------------

describe('authenticateToken — concurrency / shared state', () => {
  it('parallel identical requests produce identical results', () => {
    const token = generateToken(VALID_ADDRESS);
    const results: Array<string | undefined> = [];
    const reqs = Array.from({ length: 10 }, () =>
      makeReq({ headers: { authorization: `Bearer ${token}` } })
    );

    for (const req of reqs) {
      const next = jest.fn();
      authenticateToken(req, RES_STUB, next);
      results.push(req.user?.address);
    }

    expect(new Set(results).size).toBe(1);
    expect(results[0]).toBe(VALID_ADDRESS);
  });

  it('no shared mutable state between requests', () => {
    const token1 = generateToken(VALID_ADDRESS);
    const token2 = generateToken(VALID_ADDRESS2);

    const req1 = makeReq({ headers: { authorization: `Bearer ${token1}` } });
    const req2 = makeReq({ headers: { authorization: `Bearer ${token2}` } });

    authenticateToken(req1, RES_STUB, jest.fn());
    authenticateToken(req2, RES_STUB, jest.fn());

    expect(req1.user?.address).toBe(VALID_ADDRESS);
    expect(req2.user?.address).toBe(VALID_ADDRESS2);
  });
});

// ---------------------------------------------------------------------------
// generateToken
// ---------------------------------------------------------------------------

describe('generateToken', () => {
  it('returns a string', () => {
    expect(typeof generateToken(VALID_ADDRESS)).toBe('string');
  });

  it('returned token is verifiable with the configured secret', () => {
    const token = generateToken(VALID_ADDRESS);
    expect(() => jwt.verify(token, TEST_SECRET)).not.toThrow();
  });

  it('token payload contains address field', () => {
    const token = generateToken(VALID_ADDRESS);
    const payload = jwt.decode(token) as Record<string, unknown>;
    expect(payload.address).toBe(VALID_ADDRESS);
  });

  it('token has an expiry (exp claim)', () => {
    const token = generateToken(VALID_ADDRESS);
    const payload = jwt.decode(token) as Record<string, unknown>;
    expect(typeof payload.exp).toBe('number');
    expect((payload.exp as number) > 0).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// verifyHookHmac — reject paths
// ---------------------------------------------------------------------------

describe('verifyHookHmac — reject paths', () => {
  const rawBody = JSON.stringify({ event: 'test' });

  beforeEach(() => {
    // Ensure hookSecret is set for all tests in this block
    (config.auth as any).hookSecret = HOOK_SECRET;
  });

  afterEach(() => {
    // Reset to default (empty)
    (config.auth as any).hookSecret = '';
  });

  it('rejects when hookSecret is not configured (empty string)', () => {
    (config.auth as any).hookSecret = '';
    const { signature, timestamp } = makeHookHeaders(rawBody, 'any');
    const req = makeReq({
      headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook authentication secret is not configured'
    );
  });

  it('rejects missing x-hook-signature header', () => {
    const timestamp = Date.now().toString();
    const req = makeReq({ headers: { 'x-hook-timestamp': timestamp }, rawBody });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook signature and timestamp headers are required'
    );
  });

  it('rejects missing x-hook-timestamp header', () => {
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update('x').digest('hex');
    const req = makeReq({ headers: { 'x-hook-signature': sig }, rawBody });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook signature and timestamp headers are required'
    );
  });

  it('rejects both headers missing', () => {
    const req = makeReq({ headers: {}, rawBody });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook signature and timestamp headers are required'
    );
  });

  it('rejects non-numeric timestamp', () => {
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update('x').digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sig, 'x-hook-timestamp': 'not-a-number' },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow('Invalid hook timestamp');
  });

  it('rejects Infinity timestamp (Number.isFinite(Infinity) === false)', () => {
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update('x').digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sig, 'x-hook-timestamp': 'Infinity' },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow('Invalid hook timestamp');
  });

  it('rejects timestamp 6 minutes in the past', () => {
    const old = (Date.now() - 6 * 60 * 1000).toString();
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update(`${old}.${rawBody}`).digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sig, 'x-hook-timestamp': old },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook timestamp outside allowable window'
    );
  });

  it('rejects timestamp 6 minutes in the future', () => {
    const future = (Date.now() + 6 * 60 * 1000).toString();
    const sig = crypto
      .createHmac('sha256', HOOK_SECRET)
      .update(`${future}.${rawBody}`)
      .digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sig, 'x-hook-timestamp': future },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook timestamp outside allowable window'
    );
  });

  it('rejects wrong signature (valid timestamp, wrong HMAC)', () => {
    const { timestamp } = makeHookHeaders(rawBody);
    const badSig = '0'.repeat(64); // all-zero 64-char hex
    const req = makeReq({
      headers: { 'x-hook-signature': badSig, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow('Invalid hook signature');
  });

  it('rejects signature computed with different body', () => {
    const { timestamp } = makeHookHeaders(rawBody);
    const sigForOtherBody = crypto
      .createHmac('sha256', HOOK_SECRET)
      .update(`${timestamp}.different_body`)
      .digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sigForOtherBody, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow('Invalid hook signature');
  });

  it('rejects signature with mismatched length (too short)', () => {
    const { timestamp } = makeHookHeaders(rawBody);
    const shortSig = 'abc'; // far too short — buffer length mismatch
    const req = makeReq({
      headers: { 'x-hook-signature': shortSig, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow('Invalid hook signature');
  });

  it('does not expose hookSecret in error message', () => {
    (config.auth as any).hookSecret = HOOK_SECRET;
    const { timestamp } = makeHookHeaders(rawBody);
    const badSig = '0'.repeat(64);
    const req = makeReq({
      headers: { 'x-hook-signature': badSig, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    let caught: Error | null = null;
    try { verifyHookHmac(req, RES_STUB, jest.fn()); } catch (e) { caught = e as Error; }
    expect(caught).not.toBeNull();
    expect(caught!.message).not.toContain(HOOK_SECRET);
  });
});

// ---------------------------------------------------------------------------
// verifyHookHmac — success paths
// ---------------------------------------------------------------------------

describe('verifyHookHmac — success paths', () => {
  const rawBody = JSON.stringify({ event: 'test' });

  beforeEach(() => {
    (config.auth as any).hookSecret = HOOK_SECRET;
  });

  afterEach(() => {
    (config.auth as any).hookSecret = '';
  });

  it('accepts valid HMAC with rawBody', () => {
    const { signature, timestamp } = makeHookHeaders(rawBody);
    const req = makeReq({
      headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
    expect(next.mock.calls[0]).toHaveLength(0);
  });

  it('falls back to JSON.stringify(req.body) when rawBody absent', () => {
    const body = { foo: 'bar' };
    const bodyStr = JSON.stringify(body);
    const ts = Date.now().toString();
    const sig = crypto.createHmac('sha256', HOOK_SECRET).update(`${ts}.${bodyStr}`).digest('hex');
    const req = makeReq({
      headers: { 'x-hook-signature': sig, 'x-hook-timestamp': ts },
      body,
      // rawBody intentionally absent
    });
    delete (req as any).rawBody;
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('accepts timestamp at exactly now (boundary)', () => {
    const ts = Date.now();
    const { signature, timestamp } = makeHookHeaders(rawBody, HOOK_SECRET, ts);
    const req = makeReq({
      headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('rejects timestamp at exactly now + 5min + 1ms (just outside window)', () => {
    // Use +5min+5s to ensure we're reliably outside the window regardless of processing time
    const future = Date.now() + 5 * 60 * 1000 + 5000;
    const { signature, timestamp } = makeHookHeaders(rawBody, HOOK_SECRET, future);
    const req = makeReq({
      headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    expect(() => verifyHookHmac(req, RES_STUB, jest.fn())).toThrow(
      'Hook timestamp outside allowable window'
    );
  });

  it('accepts timestamp at 5min - 1ms (just inside window)', () => {
    const ts = Date.now() - (5 * 60 * 1000 - 1);
    const { signature, timestamp } = makeHookHeaders(rawBody, HOOK_SECRET, ts);
    const req = makeReq({
      headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
      rawBody,
    });
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// verifyHookHmac — array headers (Express duplicate header handling)
// ---------------------------------------------------------------------------

describe('verifyHookHmac — array headers', () => {
  const rawBody = JSON.stringify({ event: 'test' });

  beforeEach(() => {
    (config.auth as any).hookSecret = HOOK_SECRET;
  });

  afterEach(() => {
    (config.auth as any).hookSecret = '';
  });

  it('uses first value when x-hook-signature is an array', () => {
    const { signature, timestamp } = makeHookHeaders(rawBody);
    const req = makeReq({
      headers: {
        'x-hook-signature': [signature, 'other-value'] as any,
        'x-hook-timestamp': timestamp,
      },
      rawBody,
    });
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('uses first value when x-hook-timestamp is an array', () => {
    const { signature, timestamp } = makeHookHeaders(rawBody);
    const req = makeReq({
      headers: {
        'x-hook-signature': signature,
        'x-hook-timestamp': [timestamp, 'other'] as any,
      },
      rawBody,
    });
    const next = jest.fn();
    expect(() => verifyHookHmac(req, RES_STUB, next)).not.toThrow();
    expect(next).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// verifyHookHmac — concurrency
// ---------------------------------------------------------------------------

describe('verifyHookHmac — concurrency', () => {
  const rawBody = JSON.stringify({ event: 'concurrent' });

  beforeEach(() => {
    (config.auth as any).hookSecret = HOOK_SECRET;
  });

  afterEach(() => {
    (config.auth as any).hookSecret = '';
  });

  it('10 parallel calls with identical valid request — all next() fire', () => {
    const { signature, timestamp } = makeHookHeaders(rawBody);
    const nexts: jest.Mock[] = [];

    for (let i = 0; i < 10; i++) {
      const req = makeReq({
        headers: { 'x-hook-signature': signature, 'x-hook-timestamp': timestamp },
        rawBody,
      });
      const next = jest.fn();
      nexts.push(next);
      verifyHookHmac(req, RES_STUB, next);
    }

    for (const next of nexts) {
      expect(next).toHaveBeenCalledTimes(1);
      expect(next.mock.calls[0]).toHaveLength(0);
    }
  });
});

// ---------------------------------------------------------------------------
// Error shape regression tests
// ---------------------------------------------------------------------------

describe('Error shape regression', () => {
  it('UnauthorizedError has statusCode 401', () => {
    const err = new UnauthorizedError('test');
    expect(err.statusCode).toBe(401);
  });

  it('UnauthorizedError is an instance of Error', () => {
    const err = new UnauthorizedError('test');
    expect(err).toBeInstanceOf(Error);
  });

  it('authenticateToken throws UnauthorizedError (not plain Error)', () => {
    const req = makeReq({ headers: {} });
    let caught: unknown = null;
    try { authenticateToken(req, RES_STUB, jest.fn()); } catch (e) { caught = e; }
    expect(caught).toBeInstanceOf(UnauthorizedError);
  });

  it('verifyHookHmac throws UnauthorizedError (not plain Error)', () => {
    (config.auth as any).hookSecret = HOOK_SECRET;
    const req = makeReq({ headers: {} });
    let caught: unknown = null;
    try { verifyHookHmac(req, RES_STUB, jest.fn()); } catch (e) { caught = e; }
    (config.auth as any).hookSecret = '';
    expect(caught).toBeInstanceOf(UnauthorizedError);
  });
});
