import winston from 'winston';
import { config } from '../config';

/**
 * Redaction list for sensitive fields.
 * This is defense-in-depth: callers should not log secrets,
 * but the logger guarantees that commonly sensitive keys never reach the transport.
 */
const REFACTED_KEYS = new Set([
  'password',
  'passwordHash',
  'token',
  'accessToken',
  'refreshToken',
  'idToken',
  'authorization',
  'cookie',
  'set-cookie',
  'secret',
  'apiKey',
  'privateKey',
]);

const REDACTED_PLACEHOLDER = '[REDACTED]';

function redact(value: unknown, seen: WeakSet<object> = new WeakSet()): unknown {
  if (value === null || typeof value !== 'object') {
    return value;
  }

  if (seen.has(value as object)) {
    return '[Circular]';
  }
  seen.add(value as object);

  if (Array.isArray(value)) {
    return value.map((entry) => redact(entry, seen));
  }

  if (value instanceof Error) {
    return {
      name: value.name,
      message: value.message,
      stack: value.stack,
    };
  }

  const out: Record<string, unknown> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    if (REDACTED_KEYS.has(key.toLowerCase())) {
      out[key] = REDACTED_PLACEHOLDER;
    } else {
      out[key] = redact(entry, seen);
    }
  }
  return out;
}

/**
 * Redacts sensitive fields in a log message object.
 * Exported for testing and reuse by callers that need to sanitize payloads.
 */
export function redactSensitive(value: unknown): unknown {
  return redact(value);
}

const redactFormat = winston.format((info: winston.LogInfo) => {
  const { stack, ...rest } = info as winston.LogInfo & { stack?: string };
  const safeInfo = redact(rest) as Record<string, unknown>;
  if (typeof stack === 'string') {
    safeInfo.stack = stack;
  }
  return safeInfo as winston.LogInfo;
  })();

const logger = winston.createLogger({
  level: config.logging.level,
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.errors({ stack: true }),
    redactFormat,
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console({
      format: winston.format.combine(
        winston.format.colorize(),
        winston.format.simple()
      ),
    }),
  ],
});

export default logger;
