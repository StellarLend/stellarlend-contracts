import winston from 'winston';
import { config } from '../config';

/**
 * Structured logger for the StellarLend API.
 *
 * ## Contract / invariants (issue #2122)
 *
 * 1. **Deterministic configuration.** `LOG_LEVEL` is validated against the set
 *    of levels winston understands. A blank or unknown value falls back to
 *    `info` instead of producing a logger that silently drops records or
 *    mis-routes them.
 * 2. **No sensitive data in output.** Any metadata key that names a credential
 *    (password/token/secret/authorization/api key/JWT/seed/…) is replaced with
 *    `[REDACTED]` at every depth before the record reaches a transport. This is
 *    recursive and works for nested objects and arrays.
 * 3. **Safe under adverse input.** Circular references, `BigInt`, `Symbol`,
 *    functions, `Date`s, `Error`s, and pathologically deep or large values are
 *    serialised defensively — a bad log payload must never throw inside the
 *    logging path (which would mask the original error) or blow the stack.
 * 4. **Existing callers remain compatible.** The default export is still a
 *    winston logger; `logger.info(msg, meta)` and `logger.error(msg, err)`
 *    continue to work.
 *
 * The pure helpers below are exported so their invariants can be asserted
 * directly in focused regression tests (`src/utils/__tests__/logger.test.ts`).
 */

/** Levels recognised by winston (npm levels); single source of truth. */
export type LogLevel =
  | 'error'
  | 'warn'
  | 'info'
  | 'http'
  | 'verbose'
  | 'debug'
  | 'silly';

export const VALID_LOG_LEVELS = [
  'error',
  'warn',
  'info',
  'http',
  'verbose',
  'debug',
  'silly',
] as const satisfies readonly LogLevel[];

/** Applied when `LOG_LEVEL` is absent or invalid. */
export const DEFAULT_LOG_LEVEL: LogLevel = 'info';

/** Replacement written in place of a sensitive value. */
export const REDACTED = '[REDACTED]';

/** Markers used for non-serialisable / defensive cases. */
export const CIRCULAR = '[Circular]';
export const MAX_DEPTH_MARKER = '[MaxDepth]';

/** Bounds so a hostile or accidental payload cannot exhaust memory/stack. */
export const MAX_REDACT_DEPTH = 8;
export const MAX_ARRAY_ITEMS = 50;
export const MAX_STRING_LENGTH = 2000;

/**
 * Validate and normalise a raw `LOG_LEVEL` value.
 *
 * Trims and lower-cases (configuration is frequently set via shells/CI where
 * `INFO ` and `info` mean the same thing) and returns the fallback for anything
 * that is not a known level — including `undefined`, non-strings, and blanks.
 */
export function resolveLogLevel(raw: string | undefined | null): LogLevel {
  if (typeof raw !== 'string') {
    return DEFAULT_LOG_LEVEL;
  }

  const candidate = raw.trim().toLowerCase();
  return (VALID_LOG_LEVELS as readonly string[]).includes(candidate)
    ? (candidate as LogLevel)
    : DEFAULT_LOG_LEVEL;
}

/**
 * Normalise a metadata key into its component words so matching is not fooled
 * by casing or separators: `apiKey`, `api_key`, and `API-KEY` all yield
 * `['api', 'key']`.
 */
function keyWords(key: string): string[] {
  return key
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[^a-zA-Z0-9]+/)
    .map((word) => word.toLowerCase())
    .filter(Boolean);
}

/** Whole-key matches that must not be split (e.g. `apiKey` -> `apikey`). */
const SENSITIVE_EXACT_KEYS = new Set([
  'apikey',
  'privatekey',
  'secretkey',
  'authorization',
  'jwt',
  'mnemonic',
]);

/** Individual words that identify a credential when they appear in a key. */
const SENSITIVE_WORDS = new Set([
  'password',
  'passwd',
  'secret',
  'token',
  'authorization',
  'auth',
  'jwt',
  'credential',
  'credentials',
  'mnemonic',
  'seed',
  'cookie',
  'session',
  'signature',
  'private',
]);

/**
 * Returns true when a metadata key names sensitive data.
 *
 * Uses whole-word matching (after normalisation) so `author` is not mistaken
 * for `auth`, and `publicKey` is not treated as secret, while `apiKey`,
 * `accessToken`, and `seedPhrase` are.
 */
export function isSensitiveKey(key: string): boolean {
  const normalised = key.toLowerCase().replace(/[^a-z0-9]/g, '');
  if (SENSITIVE_EXACT_KEYS.has(normalised)) {
    return true;
  }
  return keyWords(key).some((word) => SENSITIVE_WORDS.has(word));
}

/**
 * Serialise an `Error` into a plain, redacted object.
 *
 * `message` and `stack` are preserved for diagnosability; enumerable custom
 * properties (e.g. `statusCode`) are included but redacted by key. Cycles and
 * depth are bounded.
 */
export function serializeError(
  err: Error,
  depth = 0,
  ancestors: WeakSet<object> = new WeakSet<object>(),
): Record<string, unknown> {
  const out: Record<string, unknown> = {
    name: err.name,
    message: err.message,
  };

  if (typeof err.stack === 'string') {
    out.stack = err.stack;
  }

  if (!ancestors.has(err) && depth < MAX_REDACT_DEPTH) {
    ancestors.add(err);
    try {
      for (const [key, value] of Object.entries(err)) {
        if (key in out) {
          continue;
        }
        out[key] = isSensitiveKey(key) ? REDACTED : redactSensitive(value, depth + 1, ancestors);
      }
    } finally {
      ancestors.delete(err);
    }
  }

  return out;
}

/**
 * Recursively redact sensitive keys and defensively serialise a value.
 *
 * Guarantees:
 * - never throws for any input (including cycles and unsupported types);
 * - masks every key for which {@link isSensitiveKey} is true, at any depth;
 * - bounds depth, array length, and string length.
 */
export function redactSensitive(
  value: unknown,
  depth = 0,
  ancestors: WeakSet<object> = new WeakSet<object>(),
): unknown {
  if (value === null || value === undefined) {
    return value;
  }

  switch (typeof value) {
    case 'string':
      return value.length > MAX_STRING_LENGTH
        ? `${value.slice(0, MAX_STRING_LENGTH)}...[truncated]`
        : value;
    case 'number':
    case 'boolean':
      return value;
    case 'bigint':
      // `JSON.stringify` throws on BigInt; degrade to its decimal string.
      return value.toString();
    case 'function':
    case 'symbol':
      return String(value);
    default:
      break;
  }

  if (value instanceof Date) {
    return value.toISOString();
  }

  if (value instanceof Error) {
    if (ancestors.has(value)) {
      return CIRCULAR;
    }
    return serializeError(value, depth, ancestors);
  }

  if (typeof value === 'object') {
    if (ancestors.has(value)) {
      return CIRCULAR;
    }
    if (depth >= MAX_REDACT_DEPTH) {
      return MAX_DEPTH_MARKER;
    }

    ancestors.add(value);
    try {
      if (Array.isArray(value)) {
        const items = value
          .slice(0, MAX_ARRAY_ITEMS)
          .map((item) => redactSensitive(item, depth + 1, ancestors));
        if (value.length > MAX_ARRAY_ITEMS) {
          items.push(`...${value.length - MAX_ARRAY_ITEMS} more`);
        }
        return items;
      }

      const result: Record<string, unknown> = {};
      for (const [key, nested] of Object.entries(value as Record<string, unknown>)) {
        result[key] = isSensitiveKey(key) ? REDACTED : redactSensitive(nested, depth + 1, ancestors);
      }
      return result;
    } finally {
      ancestors.delete(value);
    }
  }

  return value;
}

/** Reserved winston keys that must never be redacted or rewritten. */
const RESERVED_INFO_KEYS = new Set(['level', 'message']);

/**
 * Winston format that applies {@link redactSensitive} to every metadata field
 * on a log record (top-level merged meta plus splat arguments).
 *
 * Must run after `format.errors()` so an `Error` passed as the second argument
 * has already contributed its `stack`, and before the output format (json /
 * simple) so no transport can emit un-redacted data.
 */
export const redactFormat = winston.format((info) => {
  const record = info as unknown as Record<string | symbol, unknown>;

  for (const key of Object.keys(record)) {
    if (RESERVED_INFO_KEYS.has(key)) {
      continue;
    }
    const value = record[key];
    // Key-level redaction is essential at the top level: merged metadata
    // (e.g. `logger.info('msg', { token })`) lives here as `info.token`, so
    // the value alone cannot reveal that it must be masked.
    record[key] = isSensitiveKey(key)
      ? REDACTED
      : value instanceof Error
        ? serializeError(value)
        : redactSensitive(value);
  }

  const splat = record[Symbol.for('splat')];
  if (Array.isArray(splat)) {
    record[Symbol.for('splat')] = splat.map((item) =>
      item instanceof Error ? serializeError(item) : redactSensitive(item),
    );
  }

  return info;
});

export interface LoggerOptions {
  /** Raw log level; validated with {@link resolveLogLevel}. */
  level?: string;
  /** Force JSON output. Defaults to JSON when `NODE_ENV === 'production'`. */
  json?: boolean;
  /** Optional destination stream (used by tests to capture output). */
  stream?: NodeJS.WritableStream;
}

/**
 * Build a logger instance with the hardened format pipeline.
 *
 * Kept as a factory so tests can construct isolated loggers (custom level,
 * JSON output, captured stream) without touching the process-wide singleton.
 */
export function buildLogger(options: LoggerOptions = {}): winston.Logger {
  const useJson = options.json ?? config.server.env === 'production';

  const outputFormat = useJson
    ? winston.format.combine(winston.format.timestamp(), winston.format.json())
    : winston.format.combine(winston.format.colorize(), winston.format.simple());

  const format = winston.format.combine(
    winston.format.errors({ stack: true }),
    redactFormat(),
    outputFormat,
  );

  // winston 3.11's Console transport writes straight to `console` and ignores
  // a `stream` option, so a dedicated Stream transport is used when a caller
  // supplies one (tests capture output this way). Runtime behaviour is
  // unchanged: the default is the Console transport.
  const transport = options.stream
    ? new winston.transports.Stream({ stream: options.stream })
    : new winston.transports.Console();

  return winston.createLogger({
    level: resolveLogLevel(options.level),
    format,
    transports: [transport],
  });
}

/**
 * Application-wide logger.
 *
 * Note: the logger-level format is inherited by the console transport (no
 * transport-level `format` override), so the redaction/validation pipeline is
 * actually applied to every emitted record.
 */
const logger = buildLogger({ level: config.logging.level });

export default logger;
