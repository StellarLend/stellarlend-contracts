import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';

/**
 * Body validation middleware.
 *
 * Invariants:
 *  - The request body is replaced with the parsed, normalized value only after a successful parse.
 *  - On failure the body is left untouched and a deterministic ValidationError is forwarded
 *    to the error handler via `next(`.
 *  - Non-Zod errors are propagated unchanged so they are not mislabeled as validation failures.
 *  - Error messages include the field path but never echo the received value, so secrets are not leaked.
 */
export const validateBody =
  (schema: ZodSchema) => (req: Request, _res: Response, next: NextFunction) => {
    try {
      const parsed = schema.parse(req.body);
      // Only mutate the body after a successful parse to avoid leaving partially
      // normalized state on failure.
      req.body = parsed;
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

export const optionalStellarAddress = z.preprocess(
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
