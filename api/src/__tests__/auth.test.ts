import { authenticateToken, generateToken, AuthRequest } from '../middleware/auth';
import { UnauthorizedError } from '../utils/errors';
import { TransactionStatus } from '../types';

describe('Auth Middleware', () => {
  const response = {} as any;

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

      expect(() => authenticateToken(request, response, next)).toThrow(
        new UnauthorizedError('Invalid or expired token')
      );
      expect(next).not.toHaveBeenCalled();
    }
  });

  it('should attach decoded user for valid access token', () => {
    const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY4tOWHC3TPNR2I6NNV3ZSJ';
    const token = generateToken(address);
    const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
    const next = jest.fn();

    authenticateToken(request, response, next);

    expect(request.user).toEqual({ address, iat: expect.any(Number), exp: expect.any(Number) });
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('should be deterministic across repeated authentication attempts', () => {
    const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY4tOWHC3TPNR2I6NNV3ZSJ';
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

  it('should expose transaction status values', () => {
    expect(TransactionStatus.PENDING).toBe('pending');
    expect(TransactionStatus.SUCCESS).toBe('success');
    expect(TransactionStatus.FAILED).toBe('failed');
    expect(TransactionStatus.NOT_FOUND).toBe('not_found');
  });

  it('should keep transaction status values unique and immutable in shape', () => {
    const values = Object.values(TransactionStatus);
    expect(new Set(values).size).toBe(values.length);
    for (const value of values) {
      expect(typeof value).toBe('string');
    }
  });
});
