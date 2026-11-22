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
