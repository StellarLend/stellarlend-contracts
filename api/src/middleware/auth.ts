/**
 * Authentication middleware invariants
 *
 * @module auth
 *
 * ## authenticateToken
 * Invariant 1: Every request that passes through this middleware carries a
 *   valid, non-expired JWT signed with `config.auth.jwtSecret`.
 * Invariant 2: On success `req.user.address` is a non-empty string decoded
 *   from the token payload; `next()` is called exactly once with no arguments.
 * Invariant 3: On any failure the function throws `UnauthorizedError` (not
 *   calls `next(err)`). The Express error handler translates this to HTTP 401.
 * Invariant 4: The token value, secret, and raw header are never included in
 *   error messages or logs.
 *
 * ## verifyHookHmac
 * Invariant 5: Every request that passes has a valid HMAC-SHA256 signature
 *   computed over `<timestamp>.<rawBody>` and the timestamp is within 5 minutes
 *   of the server clock.
 * Invariant 6: If `config.auth.hookSecret` is falsy the middleware throws
 *   immediately; no request can be accepted without a configured secret.
 * Invariant 7: Signature comparison uses `crypto.timingSafeEqual` to prevent
 *   timing oracle attacks.
 * Invariant 8: `verifyHookHmac` throws synchronously; the Express error
 *   handler is responsible for returning HTTP 401.
 *
 * ## generateToken
 * Invariant 9: Returns a signed JWT containing `{ address }` with the
 *   configured expiry; callers must treat the returned string as a secret.
 */

import crypto from 'crypto';
import { Request, Response, NextFunction } from 'express';
import jwt from 'jsonwebtoken';
import { config } from '../config';
import { UnauthorizedError } from '../utils/errors';

export interface AuthRequest extends Request {
  user?: {
    address: string;
  };
  rawBody?: string;
}

const HOOK_SIGNATURE_HEADER = 'x-hook-signature';
const HOOK_TIMESTAMP_HEADER = 'x-hook-timestamp';
const HOOK_WINDOW_MS = 5 * 60 * 1000;

export const authenticateToken = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const authHeader = req.headers['authorization'];
  const token = authHeader && authHeader.split(' ')[1];

  if (!token) {
    throw new UnauthorizedError('Access token required');
  }

  try {
    const decoded = jwt.verify(token, config.auth.jwtSecret) as { address: string };
    req.user = decoded;
    next();
  } catch (error) {
    throw new UnauthorizedError('Invalid or expired token');
  }
};

export const generateToken = (address: string): string => {
  return jwt.sign({ address }, config.auth.jwtSecret, {
    expiresIn: config.auth.jwtExpiresIn,
  } as jwt.SignOptions);
};

export const verifyHookHmac = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const signatureHeader = req.headers[HOOK_SIGNATURE_HEADER];
  const timestampHeader = req.headers[HOOK_TIMESTAMP_HEADER];
  const signature = Array.isArray(signatureHeader)
    ? signatureHeader[0]
    : signatureHeader;
  const timestampValue = Array.isArray(timestampHeader)
    ? timestampHeader[0]
    : timestampHeader;

  if (!config.auth.hookSecret) {
    throw new UnauthorizedError('Hook authentication secret is not configured');
  }

  if (!signature || !timestampValue) {
    throw new UnauthorizedError('Hook signature and timestamp headers are required');
  }

  const timestamp = Number(timestampValue);

  if (!Number.isFinite(timestamp)) {
    throw new UnauthorizedError('Invalid hook timestamp');
  }

  if (Math.abs(Date.now() - timestamp) > HOOK_WINDOW_MS) {
    throw new UnauthorizedError('Hook timestamp outside allowable window');
  }

  const rawBody = req.rawBody ?? JSON.stringify(req.body ?? {});
  const payload = `${timestampValue}.${rawBody}`;
  const expectedSignature = crypto
    .createHmac('sha256', config.auth.hookSecret)
    .update(payload)
    .digest('hex');

  const signatureBuffer = Buffer.from(signature, 'hex');
  const expectedBuffer = Buffer.from(expectedSignature, 'hex');

  if (
    signatureBuffer.length !== expectedBuffer.length ||
    !crypto.timingSafeEqual(signatureBuffer, expectedBuffer)
  ) {
    throw new UnauthorizedError('Invalid hook signature');
  }

  next();
};
