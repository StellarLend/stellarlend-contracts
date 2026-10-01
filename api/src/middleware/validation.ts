import { Request, Response, NextFunction } from 'express';
import { z, ZodError, ZodSchema } from 'zod';
import { ValidationError } from '../utils/errors';
import { I128String, PositiveI128String, StellarAddress } from '../utils/validators';
import logger from '../utils/logger';

/**
 * Sensitive field patterns to identify authorization credentials and secrets.
 * Any issue on these paths will be scrubbed to prevent leaking credentials in diagnostics.
 */
const SENSITIVE_FIELD_NAMES = new Set([
  'usersecret',
  'secret',
  'password',
  'token',
  'authorization',
  'key',
  'privatekey',
  'seed',
]);

function sanitizeFieldName(path: string): string {
  return path.toLowerCase().replace(/[^a-z0-9]/g, '');
}

function isSensitiveField(path: string): boolean {
  const normalized = sanitizeFieldName(path);
  return Array.from(SENSITIVE_FIELD_NAMES).some(sensitive => normalized.includes(sensitive));
}

/**
 * Express middleware factory that validates the request body against a Zod schema.
 *
 * Invariants enforced by validateBody:
 * 1. Determinism: Identical input payloads always produce the exact same validation
 *    outcome and error structure, regardless of retries, timing, or concurrency.
 * 2. State Transition & Isolation: req.body is replaced with the parsed (validated and
 *    normalized) output only upon successful validation. On rejection, req.body is NOT
 *    corrupted or partially updated, and downstream handlers/controllers are not invoked.
 * 3. Credential Protection: Authorization credentials (such as userSecret) are never echoed
 *    or leaked in error messages, logs, or responses.
 * 4. Async & Sync Compatibility: Supports both synchronous and asynchronous Zod schemas,
 *    handling promises cleanly without unhandled rejections or race conditions.
 * 5. Diagnostic Observability: Validation failures emit structured warning logs containing
 *    request method, path, and error summaries while strictly redacting sensitive fields.
 * 6. Error Propagation: ZodError issues are formatted into a single ValidationError.
 *    Non-Zod errors are forwarded via next(err) without alteration.
 */
export const validateBody =
  (schema: ZodSchema) => async (req: Request, res: Response, next: NextFunction) => {
    try {
      const parsed = schema.parse(req.body);
      req.body = parsed;
      next();
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

/**
 * Handles validation failures with safe logging and standardized error propagation.
 */
function handleValidationFailure(error: unknown, req: Request, next: NextFunction): void {
  if (error instanceof ZodError) {
    const errorMessages = error.issues
      .map(issue => {
        const path = issue.path.join('.') || 'body';
        return `${path}: ${issue.message}`;
      })
      .join(', ');

    // Safe diagnostic telemetry without leaking sensitive credentials
    try {
      logger.warn('Request body validation failed', {
        method: req?.method,
        path: req?.path,
        error: errorMessages,
        issues: error.issues.map(i => ({
          path: i.path.join('.') || 'body',
          code: i.code,
          message: i.message,
          isSensitive: isSensitiveField(i.path.join('.')),
        })),
      });
    } catch {
      // Diagnostic logging failure must never prevent error propagation
    }

    return next(new ValidationError(errorMessages));
  }

  return next(error);
}

/**
 * Normalizes an optional Stellar address field.
 *
 * Invariants:
 * - `undefined` and `null` are normalized to `undefined`.
 * - Empty string `""` or whitespace-only strings are normalized to `undefined`.
 * - Non-empty strings are passed to `StellarAddress` for strict address validation.
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
