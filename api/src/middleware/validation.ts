import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';

/**
 * Validates the request body against a provided Zod schema.
 * Enforces strong input boundaries by stripping unrecognized fields
 * and properly handling async validation rules.
 *
 * Invariants:
 * 1. Invalid inputs deterministically fail before reaching controllers.
 * 2. Unrecognized fields are stripped, preventing mass-assignment.
 * 3. Supports asynchronous validations (e.g. state-transition checks).
 * 4. Fails safely without leaking sensitive payload data.
 */
export const validateBody =
  (schema: ZodSchema) => async (req: Request, res: Response, next: NextFunction) => {
    try {
      req.body = await schema.parseAsync(req.body);
      return next();
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
