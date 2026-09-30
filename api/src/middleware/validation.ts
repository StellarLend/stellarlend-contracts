import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';

export const validateBody =
  (schema: ZodSchema) => (req: Request, res: Response, next: NextFunction) => {
    try {
      req.body = schema.parse(req.body);
      next();
    } catch (error) {
      if (error instanceof ZodError) {
        const errorMessages = error.issues
          .map(issue => `${issue.path.join('.') || 'body'}: ${issue.message}`)
          .join(', ');
        return next(new ValidationError(errorMessages));
      }

      return next(error);
    }
  };

const optionalStellarAddress = z.preprocess(
  value => (value === '' ? undefined : value),
  StellarAddress.optional()
);

export const lendingRequestSchema = z.object({
  userAddress: StellarAddress,
  amount: PositiveI128String,
  assetAddress: optionalStellarAddress,
  userSecret: z.string().trim().min(1, 'User secret is required'),
});

export const depositValidation = [validateBody(lendingRequestSchema)];
export const borrowValidation = [validateBody(lendingRequestSchema)];
export const repayValidation = [validateBody(lendingRequestSchema)];
export const withdrawValidation = [validateBody(lendingRequestSchema)];

export { I128String, StellarAddress };
