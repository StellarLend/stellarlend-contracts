import { Request, Response, NextFunction } from 'express';
const { ApiError } = require('../utils/errors');
const logger = require('../utils/logger').default || require('../utils/logger');

/**
 * Error handler middleware.
 *
 * Invariants:
 * - Client errors (4xx) expose the original message only when it is an ApiError.
 * - Server errors (5xx) and unknown errors never leak the original message or stack.
 * - Known ApiError status codes are preserved; out-of-range codes fall back to 500.
 * - Malformed JSON bodies (SyntaxError with `body`) are reported as 400.
 * - Handler is pure and deterministic: no shared mutable state, safe to retry/concurrently call.
 */

export interface ErrorResponseBody {
  success: false;
  error: string;
}

export const DEFAULT_SERVER_ERROR_MESSAGE = 'Internal server error';

const isValidStatusCode = (value: unknown): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= 400 && value <= 599;

export const errorHandler = (err: Error, _req: Request, res: Response, next: NextFunction) => {
  const invalidJson = err instanceof SyntaxError && 'body' in err;
  // Only operational client errors have messages intended for callers. An invalid
  // status or a server failure must never turn into a success or expose internals.
  const publicApiError =
    err instanceof ApiError &&
    err.isOperational === true &&
    Number.isInteger(err.statusCode) &&
    err.statusCode >= 400 &&
    err.statusCode < 500 &&
    typeof err.message === 'string' &&
    err.message.length > 0;
  const statusCode = invalidJson ? 400 : publicApiError ? err.statusCode : 500;
  const message = invalidJson
    ? 'Invalid JSON body'
    : publicApiError
      ? err.message
      : 'Internal server error';

  // Error messages, stacks, and request fields may contain user supplied secrets.
  // The category and status diagnose the failure without logging those fields.
  logger.error('Request failed', {
    category: invalidJson ? 'invalid_json' : publicApiError ? 'client_error' : 'server_error',
    statusCode,
  });

  // Express owns recovery once headers are committed; sending again corrupts the response.
  if (res.headersSent) {
    return next(err instanceof Error ? err : new Error('Unknown error'));
  }

  return res.status(statusCode).json({
    success: false,
    error: message,
  });
};

export default errorHandler;
