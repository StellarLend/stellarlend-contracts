import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';
import logger from '../utils/logger';

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

/**
 * Translate a Zod failure into the canonical `ValidationError` and forward it.
 *
 * Only the field *path* and the schema-authored *message* are used. Zod's
 * `message` is written by the schema author and never contains the received
 * value, so a `userSecret`/token in the body cannot be echoed back to the
 * client or into the logs. `req.body` is deliberately left untouched here: the
 * controller must not observe a partially normalized body.
 */
const handleValidationFailure = (
  error: unknown,
  req: Request,
  next: NextFunction
): void => {
  if (error instanceof ZodError) {
    const errorMessages = error.issues
      .map(issue => `${issue.path.join('.') || 'body'}: ${issue.message}`)
      .join(', ');

    // The message is field-level and value-free, so it is safe to log; the body
    // itself is never included.
    logger.warn('Request body validation failed', {
      method: req.method,
      path: req.path,
      error: errorMessages,
    });

    return next(new ValidationError(errorMessages));
  }

  // Anything that is not a schema violation (a programmer error, a transport
  // failure, a rejected promise) is propagated untouched so the central error
  // handler can classify it correctly.
  return next(error as Error);
};

export const validateBody =
  (schema: ZodSchema) => (req: Request, _res: Response, next: NextFunction) => {
    try {
      const parsed = schema.parse(req.body);
      // Only mutate the body after a successful parse to avoid leaving partially
      // normalized state on failure.
      req.body = parsed;
      return next();
    } catch (error) {
      if (
        error instanceof Error &&
        error.message.includes('Encountered Promise during synchronous parse') &&
        typeof (schema as any).parseAsync === 'function'
      ) {
        (schema as any)
          .parseAsync(req.body)
          .then((validatedBody: any) => {
            req.body = validatedBody;
            next();
          })
          .catch((asyncError: unknown) => {
            handleValidationFailure(asyncError, req, next);
          });
        return;
      }

      handleValidationFailure(error, req, next);
    }
  };

export const optionalStellarAddress = z.preprocess(
  value => (value === '' ? undefined : value),
  StellarAddress.optional()
);

/**
 * Schema for core lending operations (deposit, borrow, repay, withdraw).
 *
 * Invariants:
 * - `userAddress`: Must be a valid Stellar account (G...) or contract (C...) address.
 * - `amount`: Must be a positive signed 128-bit integer string (1 <= amount <= i128::MAX).
 *   Rejects zero, negative amounts, decimals, scientific notation, and non-numeric values.
 * - `assetAddress`: Optional Stellar address. Normalized via optionalStellarAddress.
 * - `userSecret`: Required transaction signing authorization credential.
 *   Must be non-empty and non-whitespace. Raw secret values are never echoed in errors.
 */
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

export { I128String, PositiveI128String, StellarAddress };
