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

  it('should attach decoded user for valid access token', () => {
    const address = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
    const token = generateToken(address);
    const request = { headers: { authorization: `Bearer ${token}` } } as AuthRequest;
    const next = jest.fn();

    authenticateToken(request, response, next);

    expect(request.user).toEqual({ address, iat: expect.any(Number), exp: expect.any(Number) });
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('should expose transaction status values', () => {
    expect(TransactionStatus.PENDING).toBe('pending');
    expect(TransactionStatus.SUCCESS).toBe('success');
    expect(TransactionStatus.FAILED).toBe('failed');
    expect(TransactionStatus.NOT_FOUND).toBe('not_found');
  });
});
