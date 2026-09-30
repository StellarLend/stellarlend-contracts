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
