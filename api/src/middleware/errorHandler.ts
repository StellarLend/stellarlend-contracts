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

export const errorHandler = (
  error: Error,\n  req: Request,
  res: Response,
  _next: NextFunction
) => {
  const safePath = typeof req?.path === 'string' ? req.path : 'unknown';
  const safeMethod = typeof req?.method === 'string' ? req.method : 'unknown';

  const isSyntaxError =
    error instanceof SyntaxError && 'body' in (error as unknown as object);

  const isApiError = error instanceof ApiError;

  // Observability: log structured context. Stack is only logged for unexpected
  // errors to avoid noise and to keep client errors cheap to diagnose.
  logger.error('Error occurred:', {
    error: error instanceof Error ? error.message : String(error),
    stack: error instanceof Error ? error.stack : undefined,
    path: safePath,
    method: safeMethod,
    statusCode: isApiError ? (error as ApiError).statusCode : isSyntaxError ? 400 : 500,
  });

  if (isSyntaxError) {
    return res.status(400).json({
      success: false,
      error: (error as Error).message,
    } as ErrorResponseBody);
  }

  if (isApiError) {
    const apiError = error as ApiError;
    const statusCode = isValidStatusCode(apiError.statusCode) ? apiError.statusCode : 500;
    const message =
      statusCode >= 500
        ? DEFAULT_SERVER_ERROR_MESSAGE
        : apiError.message || 'Request failed';

    return res.status(statusCode).json({
      success: false,
      error: message,
    } as ErrorResponseBody);
  }

  return res.status(500).json({
    success: false,
    error: DEFAULT_SERVER_ERROR_MESSAGE,
  } as ErrorResponseBody);
};

export default errorHandler;
