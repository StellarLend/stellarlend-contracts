import { Request, Response, NextFunction } from 'express';
import { ApiError } from '../utils/errors';
import logger from '../utils/logger';

/**
 * Express error handling middleware.
 *
 * Invariants:
* -  Every error response has a deterministic shape: `{ success: false, error: string }`.
 * -  Internal error details (stack traces, messages) are never leaked to clients for unexpected
 *    errors; only sanitized messages for known `ApiError` and JSON parsing failures are returned.
 * -  Once a status code has been committed to the response (headers sent), the middleware
 *    delegates to the next error handler instead of attempting to write a second response.
 * -  The middleware is pure with respect to the request: repeated invocations with the same
 *    inputs produce the same outcome, so retries and concurrent execution cannot corrupt state.
 */

export const errorHandler = (
  error: Error,
  req: Request,
  res: Response,
  next: NextFunction
): void | Response => {
  // Defensive: normalize non-Error thrown values so downstream logging and branching are safe.
  const normalizedError =
    error instanceof Error
      ? error
      : new Error(typeof error === 'string' ? error : 'Unknown error');

  // Log full details internally for diagnosability, but never expose them to clients.
  logger.error('Error occurred:', {
    error: normalizedError.message,
    stack: normalizedError.stack,
    path: req.path,
    method: req.method,
  });

  // If the response has already been committed, we cannot write a new one.
  // Delegate to Express' default error handler to avoid double-send and corrupted output.
  if (res.headersSent) {
    return next(normalizedError);
  }

  // JSON body parsing failures are client errors and safe to report as 400.
  if (normalizedError instanceof SyntaxError && 'body' in normalizedError) {
    return res.status(400).json({
      success: false,
      error: normalizedError.message,
    });
  }

  // Known, explicitly classified API errors carry their own status code and safe message.
  if (normalizedError instanceof ApiError) {
    const statusCode =
      Number.isInteger(normalizedError.statusCode) &&
      normalizedError.statusCode >= 400 &&
      normalizedError.statusCode <= 599
        ? normalizedError.statusCode
        : 500;

    return res.status(statusCode).json({
      success: false,
      error: normalizedError.message,
    });
  }

  // Unexpected errors: return a generic message to avoid leaking internal details.
  return res.status(500).json({
    success: false,
    error: 'Internal server error',
  });
};
