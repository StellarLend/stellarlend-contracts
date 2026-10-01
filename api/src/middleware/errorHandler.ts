import { Request, Response, NextFunction } from 'express';
import { ApiError } from '../utils/errors';
import logger from '../utils/logger';

/**
 * Global Error Handler
 *
 * Invariants:
 * 1. Every failure is logged exactly once, and only with a non-sensitive
 *    category + status pair. Error messages, stacks, bodies, and headers are
 *    never logged or echoed: under Node >= 20 `JSON.parse` failures embed a
 *    snippet of the submitted body in `err.message`, so logging the message
 *    would write caller secrets (e.g. `userSecret`) into the log sink.
 * 2. The response shape is always `{ success: false, error }`.
 * 3. HTTP status codes are deterministic for each error class:
 *    - Malformed JSON body (`SyntaxError` carrying `body`) -> 400
 *    - Operational `ApiError` with a 4xx status -> that status
 *    - Everything else -> 500 with a generic message
 * 4. The handler is total: `err` may be `null`, `undefined`, a primitive, or a
 *    non-`Error` rejection. It must never throw, because a throw here would
 *    replace a diagnosable 4xx/5xx with Express's opaque 500 page.
 * 5. Headers already sent (partial failure mid-stream) are delegated to
 *    Express's default handler so the response is never written twice.
 */

export interface ErrorResponseBody {
  success: false;
  error: string;
}

/** Single, non-sensitive fallback message for every unexpected failure. */
export const DEFAULT_SERVER_ERROR_MESSAGE = 'Internal server error';

/** Returned for a body that could not be parsed as JSON. */
export const INVALID_JSON_MESSAGE = 'Invalid JSON body';

/**
 * Only client (4xx) operational `ApiError`s may surface their own message.
 * 5xx messages and non-operational errors are internal details by definition.
 */
const isPublicClientError = (err: ApiError): boolean =>
  err.isOperational === true &&
  Number.isInteger(err.statusCode) &&
  err.statusCode >= 400 &&
  err.statusCode < 500 &&
  typeof err.message === 'string' &&
  err.message.length > 0;

export const errorHandler = (
  err: Error,
  _req: Request,
  res: Response,
  next: NextFunction
): void | Response => {
  // Classify before touching `err.*`: a non-Error rejection has no such fields.
  const invalidJson = err instanceof SyntaxError && 'body' in err;
  const publicClientError = err instanceof ApiError && isPublicClientError(err);

  const statusCode = invalidJson ? 400 : publicClientError ? (err as ApiError).statusCode : 500;
  const message = invalidJson
    ? INVALID_JSON_MESSAGE
    : publicClientError
      ? (err as ApiError).message
      : DEFAULT_SERVER_ERROR_MESSAGE;

  // Invariant 1: category + status diagnose the failure without reproducing the
  // request payload, headers, or error message that may embed caller secrets.
  logger.error('Request failed', {
    category: invalidJson ? 'invalid_json' : publicClientError ? 'client_error' : 'server_error',
    statusCode,
  });

  // Invariant 5: Express owns recovery once headers are committed; sending
  // again would corrupt the response.
  if (res.headersSent) {
    return next(err instanceof Error ? err : new Error('Unknown error'));
  }

  const body: ErrorResponseBody = { success: false, error: message };
  return res.status(statusCode).json(body);
};

export default errorHandler;
