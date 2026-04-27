import { body, validationResult } from 'express-validator';
import { Request, Response, NextFunction } from 'express';
import { ValidationError } from '../utils/errors';

export const validateRequest = (req: Request, res: Response, next: NextFunction) => {
  const errors = validationResult(req);
  if (!errors.isEmpty()) {
    const errorMessages = errors.array().map(err => err.msg).join(', ');
    throw new ValidationError(errorMessages);
  }
  next();
};

export const depositValidation = [
  body('userAddress').isString().notEmpty().withMessage('User address is required'),
  body('amount').isString().notEmpty().withMessage('Amount is required')
    .custom((value) => {
      const num = BigInt(value);
      return num > 0n;
    }).withMessage('Amount must be greater than zero'),
  body('assetAddress').optional().isString(),
  body('userSecret').isString().notEmpty().withMessage('User secret is required'),
  validateRequest,
];

export const borrowValidation = [
  body('userAddress').isString().notEmpty().withMessage('User address is required'),
  body('amount').isString().notEmpty().withMessage('Amount is required')
    .custom((value) => {
      const num = BigInt(value);
      return num > 0n;
    }).withMessage('Amount must be greater than zero'),
  body('assetAddress').optional().isString(),
  body('userSecret').isString().notEmpty().withMessage('User secret is required'),
  validateRequest,
];

export const repayValidation = [
  body('userAddress').isString().notEmpty().withMessage('User address is required'),
  body('amount').isString().notEmpty().withMessage('Amount is required')
    .custom((value) => {
      const num = BigInt(value);
      return num > 0n;
    }).withMessage('Amount must be greater than zero'),
  body('assetAddress').optional().isString(),
  body('userSecret').isString().notEmpty().withMessage('User secret is required'),
  validateRequest,
];

export const withdrawValidation = [
  body('userAddress').isString().notEmpty().withMessage('User address is required'),
  body('amount').isString().notEmpty().withMessage('Amount is required')
    .custom((value) => {
      const num = BigInt(value);
      return num > 0n;
    }).withMessage('Amount must be greater than zero'),
  body('assetAddress').optional().isString(),
  body('userSecret').isString().notEmpty().withMessage('User secret is required'),
  validateRequest,
];
