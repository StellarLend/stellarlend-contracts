import express, { Application, NextFunction, Request, Response } from 'express';
import helmet from 'helmet';
import cors from 'cors';
import rateLimit from 'express-rate-limit';
import { config } from './config';
import lendingRoutes from './routes/lending.routes';
import healthRoutes from './routes/health.routes';
import { errorHandler } from './middleware/errorHandler';
import logger from './utils/logger';

const app: Application = express();

/**
 * Invariants enforced by this module:
 *
 * 1. Request bodies are parsed deterministically and the raw body is preserved
 *    for signature verification.
 *    - The size boundary is explicit (`BODY_LIMIT`), not an implicit library
 *      default, so the 413 threshold is the same on every deployment.
 *    - `verify` is attached to *both* JSON and urlencoded parsers. Otherwise a
 *      form-encoded hook request would be verified against a re-serialized
 *      object and a correctly signed request would be rejected.
 * 2. Rate limiting is applied to all /api/ routes with a deterministic
 *    configuration, and its rejection is emitted in the same JSON envelope as
 *    every other error this module produces.
 * 3. Unknown routes (under /api/ and elsewhere) return a consistent 404 JSON
 *    response.
 * 4. Body-parser failures are mapped to a fixed status/message table, so an
 *    oversized body, too many form parameters, or an unsupported charset can
 *    never be misreported as a 500.
 * 5. All other errors are routed through the central error handler, which
 *    never leaks internal details.
 * 6. Nothing logged here can contain caller-supplied data. Error *messages*
 *    and stacks are excluded because a parser failure message can embed a
 *    fragment of the submitted body (which carries `userSecret`). Only the
 *    closed-vocabulary `type` discriminator, the resolved status, and the
 *    request's own method/path are logged.
 */

/**
 * Maximum accepted request body size, applied to both body parsers.
 *
 * Matches body-parser's own default (100kb) so this is behaviour-preserving;
 * it is spelled out here so the 413 boundary is an asserted invariant rather
 * than an implementation detail of a dependency.
 */
export const BODY_LIMIT = '100kb';

/** Body returned by every error this module emits itself. */
export interface AppErrorBody {
  success: false;
  error: string;
}

/** Message returned once a client exceeds the rate limit. */
export const RATE_LIMIT_MESSAGE = 'Too many requests from this IP, please try again later.';

/**
 * Deterministic mapping of body-parser failure `type`s to client responses.
 *
 * body-parser (via `http-errors`) attaches `type`, `status`, and `statusCode`
 * to every failure it raises. Only `type` is consulted: it is a fixed
 * vocabulary chosen by the parser, so it can never echo caller data, whereas a
 * `message` can. An unmapped `type` is deliberately *not* given a status here
 * — it falls through to the central error handler, which fails closed with a
 * 500 rather than guessing.
 *
 * The `message` values are static strings; the parser's own message is never
 * forwarded.
 */
export const PARSER_FAILURES: Readonly<Record<string, AppErrorBody & { status: number }>> = {
  'entity.too.large': { status: 413, success: false, error: 'Payload too large' },
  'parameters.too.many': { status: 413, success: false, error: 'Too many parameters' },
  'request.size.invalid': { status: 413, success: false, error: 'Payload too large' },
  'charset.unsupported': { status: 415, success: false, error: 'Unsupported charset' },
  'encoding.unsupported': { status: 415, success: false, error: 'Unsupported content encoding' },
};

/**
 * Resolve a caught error to a deterministic client response, or `null` when the
 * error did not originate in a body parser and must be handled downstream.
 *
 * Returns `null` for an unknown/absent `type` so classification never guesses.
 */
export function resolveParserFailure(err: unknown): AppErrorBody & { status: number } | null {
  const type = (err as { type?: unknown } | null | undefined)?.type;
  if (typeof type !== 'string') {
    return null;
  }
  return Object.prototype.hasOwnProperty.call(PARSER_FAILURES, type) ? PARSER_FAILURES[type] : null;
}

// Security headers and CSRP protection.
app.use(helmet());

app.use(cors());

/**
 * Capture the exact bytes that arrived on the wire.
 *
 * Hook signatures are computed over these bytes, so they must be the raw
 * payload rather than a re-serialized object: JSON key order and whitespace
 * are not preserved by `JSON.stringify`, and a signature computed over either
 * form must not be interchangeable with the other.
 */
const captureRawBody = (req: Request, _res: Response, buf: Buffer): void => {
  (req as Request & { rawBody?: string }).rawBody = buf.toString('utf8');
};

app.use(express.json({ limit: BODY_LIMIT, verify: captureRawBody }));
app.use(express.urlencoded({ extended: true, limit: BODY_LIMIT, verify: captureRawBody }));

/**
 * Resolve the rate-limit bucket key for a request.
 *
 * Invariant: the key is always a non-empty string, and two clients are only
 * ever merged when the platform genuinely cannot distinguish them. The `||`
 * chain is exhaustive on purpose — without the final literal, a socket with no
 * `ip` and no `remoteAddress` would resolve to `undefined` and collapse every
 * such client into one shared bucket.
 *
 * Extracted as a pure function so each branch is directly assertable.
 */
export function rateLimitKey(req: {
  ip?: string;
  socket?: { remoteAddress?: string };
}): string {
  return req.ip || req.socket?.remoteAddress || 'unknown';
}

const limiter = rateLimit({
  windowMs: config.rateLimit.windowMs,
  max: config.rateLimit.maxRequests,
  message: RATE_LIMIT_MESSAGE,
  standardHeaders: true,
  // The rejection is written as JSON (not the library default of `text/html`)
  // so a client parsing the documented error envelope never has to special-case
  // a content type. `Retry-After` and the `RateLimit-*` headers are still set
  // by the middleware before this handler runs.
  handler: (_req: Request, res: Response) => {
    res.status(429).json({ success: false, error: RATE_LIMIT_MESSAGE } satisfies AppErrorBody);
  },
  keyGenerator: (req: Request) => rateLimitKey(req),
});

app.use('/api/', limiter);

app.use('/api/health', healthRoutes);
app.use('/api/lending', lendingRoutes);

// Explicit 404 for unknown /api/ routes so clients get a deterministic JSON response.
app.use('/api/', (_req: Request, res: Response) => {
  res.status(404).json({ success: false, error: 'Not Found' } satisfies AppErrorBody);
});

// Explicit 404 for any other unknown route.
app.use((_req: Request, res: Response) => {
  res.status(404).json({ success: false, error: 'Not Found' } satisfies AppErrorBody);
});

// Centralized error handling must be last so it can catch downstream failures.
app.use((err: unknown, req: Request, res: Response, next: NextFunction) => {
  // Body-parser failures are resolved here, before the central handler, because
  // they carry their own status and must never be collapsed into a 500.
  const parserFailure = resolveParserFailure(err);

  if (parserFailure) {
    // `type` is a closed vocabulary, so it is safe to log; the parser's message
    // is not (it can embed a fragment of the submitted body).
    logger.warn('Request body rejected by parser', {
      type: (err as { type: string }).type,
      statusCode: parserFailure.status,
      path: req.path,
      method: req.method,
    });

    return res.status(parserFailure.status).json({
      success: false,
      error: parserFailure.error,
    });
  }

  // The central handler classifies, logs, and redacts. Nothing is logged here
  // so a failure is never reported twice and no message is duplicated into a
  // log sink that may not redact it.
  return errorHandler(err as Error, req, res, next);
});

export default app;
