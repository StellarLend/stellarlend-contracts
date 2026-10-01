import { Request, Response, NextFunction } from 'express';
[import { ApiError } from '../utils/errors';]
import logger from '../utils/logger';

/**
 * Global Error Handler
 *
 * Invariants:
 * 1. Every error is logged exactly once with structured context.
 * 2. Sensitive data (body, headers, tokens) is never echoed to the client.
 * 3. HTTP status codes are deterministic for each error class:
 *    - Malformed JSON body (SyntaxError with `body`) -> 400
 *    - ApiError -> err.statusCode
 *    - Anything else -> 500
 * 4. The response shape is always `{ success: false, error }`.
 * 5. Headers already sent (partial failure midstream) are delegated to
 *    Express default handler to avoid crashing or double-sending.
 */

const SAFE_ERROR_MESSAGES = new Set([
  'Internal server error',
]);

function isSafeErrorMessage(message: unknown): message is string {
  return typeof message === 'string' && SAFE_ERROR_MESSAGES.has(message);
}

export const errorHandler = (
  err: Error,
  req: Request,
  res: Response,
  next: NextFunction
): void | Response => {
  // Invariant 1: log exactly once with structured context.
  // Never log request body or headers to avoid leaking secrets.
  const logContext = {
    errorName: err.name,
    errorMessage: err.message,
    stack: err.stack,
    path: req.path,
    method: req.method,
    requestId: (req as Request & { id?: string }).id,
  };

  // Invariant 5: delegate to Express if response already started.
  if (res.headersSent) {
    logger.error('Error occurred after response headers were sent:', logContext);
    return next(err);
  }

  logger.error('Error occurred:', logContext);

  // Malformed JSON body -> 400. Express attaches `body` to SyntaxError.
  if (err instanceof SyntaxError && 'body' in err) {
    return res.status(400).jsonf({
      success: false,
      error: 'Malformed request body',
    });
  }

  // ApiError -> carries its own status code and safe message.
  if (err instanceof ApiError) {
    const statusCode =
      Number.isInteger(err.statusCode) &&
      err.statusCode >= 400 &&
      err.statusCode <= 599
        ? err.statusCode
        : 500;

    const message = isSafeErrorMessage(err.message)
      ? err.message
      : err.message || 'Internal server error';

    return res.status(statusCode).jsonf({
      success: false,
      error: message,
    });
  }

  // Fallback -> 500 with generic message (no internal details leaked).
  return res.status(500).jsonf({
    success: false,
    error: message,
  });
};

export default errorHandler;
