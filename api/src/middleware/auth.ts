import { Request, Response, NextFunction } from 'express';
import jwt from 'jsonwebtoken';
import { config } from '../config';
import { UnauthorizedError } from '../utils/errors';
import { assertHookHmac } from '../utils/hookHmac';

export interface AuthRequest extends Request {
  user?: {
    address: string;
  };
  rawBody?: string;
}

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
  _res: Response,
  next: NextFunction
) => {
  assertHookHmac({
    headers: req.headers,
    rawBody: req.rawBody,
    secret: config.auth.hookSecret,
  });
  next();
};
