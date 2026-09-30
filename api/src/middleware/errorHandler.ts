import { Request, Response, NextFunction } from 'express';
[import { ApiError } from '../utils/errors';
import logger from '../utils/logger';

/**
 * Error handling middleware.
 *
 * Invariants:
 * - Always responds with a JSON body of the shape `success: false, error: string`
 *   with a valid HTTP status code.
 * - Never exposes internal error messages or stack traces to clients for
 *   unexpected errors.
 * - ApiError status codes are clamped to the valid HTTP range [400, 599]
 *   so a malformed error cannot produce an invalid response.
 * - Malformed error objects (null, undefined, non-Error) are handled
 *   defensively without throwing.
 * - If the response has already been sent, delegate to the next error
 *   handler instead of attempting to write a second response.
 */

const MINIMUM_STATUS_CODE = 400;
const MAXIMUM_STATUS_CODE = 599;
const DEFAULT_STATUS_CODE = 500;
const DEFAULT_MESSAGE = 'Internal server error';

function isValidStatusCode(code: unknown): code is number {
  return (
    typeof code === 'number' &&
    Number.isInteger(code) &&
    code >= MINIMUM_STATUS_CODE &&
    code <= MAXIMUM_STATUS_CODE
  );
}

function normalizeError(raw: unknown): Error {
  if (raw instanceof Error) {
    return raw;
  }
  if (typeof raw === 'string') {
    return new Error(raw);
  }
  try {
    return new Error(JSON.stringify(raw));
  } catch {
    return new Error('Unknown error');
  }
}

export const errorHandler = (
  err: unknown,
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const normalized = normalizeError(err);

  // Observability: log structured context without leaking to the client.
  try {
    logger.error('Error occurred:', {
      error: normalized.message,
      stack: normalized.stack,
      path: req.path,
      method: req.method,
    });
  } catch {
    // Logging must never break error response generation.
  }

 // If the response has already been committed, delegate to the next
 // error handler to avoid 'Cannot set headers after they are sent'.
 if (res.headersSent) {
    return next(normalized);
 }

 // JSON parsing errors from body-parser carry a 'body' property and are
 // client errors (400), not server errors.
 if (normalized instanceof SyntaxError && 'body' in (normalized as SyntaxError & { body?: unknown })) {
    return res.status(400).json({
      success: false,
      error: normalized.message,
    });
  }

 if (normalized instanceof ApiError) {
    const statusCode = isValidStatusCode(normalized.statusCode)
      ? normalized.statusCode
      : DEFAULT_STATUS_CODE;
    return res.status(statusCode).json({
      success: false,
      error: normalized.message,
    });
  }

 return res.status(DEFAULT_STATUS_CODE).json({
    success: false,
    error: DEFAULT_MESSAGE,
  });
};
