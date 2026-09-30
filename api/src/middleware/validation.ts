import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';

/**
 * Invariants enforced by validateBody:
 * - The request body is replaced with the parsed (and thus validated/normalized)
 *   value only on success. On failure the original body is left untouched so
 *   downstream handlers never observe partially-validated state.
 * - Validation is deterministic: the same input always yields the same
 *   success/failure outcome and the same error message ordering.
 * - Zod issues are surfaced as a single ValidationError with a stable,
 *   path-prefixed message. Raw Zod internals are never leaked to callers.
 * - Non-Zod errors (e.g. thrown by a custom refinement) are forwarded
 *   unchanged so unexpected failures remain diagnosable.
 */
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

/**
 * Normalizes an optional Stellar address field.
 *
 * Invariants:
 * - `undefined` and `null` are treated as "not provided".
 * - An empty or whitespace-only string is treated as "not provided" so that
 *   clients sending `""` for an optional field are not rejected.
 * - Any other value is passed through unchanged to `StellarAddress` for
 *   strict validation. This keeps the schema deterministic for boundary
 *   inputs (empty string, whitespace, missing key) without weakening the
 *   underlying address validation.
 */
const optionalStellarAddress = z.preprocess(
  value => {
    if (value === undefined || value === null) return undefined;
    if (typeof value === 'string' && value.trim() === '') return undefined;
    return value;
  },
  StellarAddress.optional()
);

/**
 * Schema for lending-related requests (deposit/borrow/repay/withdraw).
 *
 * Invariants:
 * - `userAddress` and `assetAddress` (when provided) must be valid Stellar
 *   addresses; `assetAddress` is optional and normalized via
 *   `optionalStellarAddress`.
 * - `amount` must be a positive i128 string; zero and negative values are
 *   rejected to prevent no-op or unsafe state transitions.
 * - `userSecret` must be a non-empty, trimmed string. Trimming happens before
 *   the min-length check so whitespace-only secrets are rejected.
 * - The schema is strict about unknown keys being ignored (default Zod
 *   behavior) but never silently coerces invalid values into valid ones.
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

export { I128String, StellarAddress };
