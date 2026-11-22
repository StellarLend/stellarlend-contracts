import { Writable } from 'stream';
import winston from 'winston';
import defaultLogger, {
  buildLogger,
  CIRCULAR,
  DEFAULT_LOG_LEVEL,
  isSensitiveKey,
  MAX_DEPTH_MARKER,
  MAX_STRING_LENGTH,
  REDACTED,
  redactSensitive,
  resolveLogLevel,
  serializeError,
  VALID_LOG_LEVELS,
} from '../logger';

/**
 * Focused regression suite for `api/src/utils/logger.ts` (issue #2122).
 *
 * Covers the module's invariants directly:
 *  - configuration validation is deterministic for valid, invalid, and blank
 *    `LOG_LEVEL` values;
 *  - sensitive metadata is redacted at every depth, in objects and arrays;
 *  - hostile/adverse payloads (cycles, BigInt, symbols, deep/large values)
 *    cannot throw or exhaust resources inside the logging path;
 *  - the public logger still emits `logger.info(msg, meta)` /
 *    `logger.error(msg, err)` records.
 */

const CANARY = 'S3CR3T-CANARY-DO-NOT-LEAK-XYZ';

/** A synchronous writable that captures every chunk written by winston. */
function captureStream(): { stream: NodeJS.WritableStream; text: () => string; records: () => unknown[] } {
  const chunks: string[] = [];
  const stream = new Writable({
    write(chunk, _encoding, callback) {
      chunks.push(chunk.toString());
      callback();
    },
  });

  return {
    stream,
    text: () => chunks.join(''),
    records: () =>
      chunks
        .join('')
        .split('\n')
        .filter((line) => line.trim().length > 0)
        .map((line) => JSON.parse(line) as unknown),
  };
}

describe('resolveLogLevel — validation', () => {
  it('accepts every valid level, case-insensitively and trimmed', () => {
    for (const level of VALID_LOG_LEVELS) {
      expect(resolveLogLevel(level)).toBe(level);
      expect(resolveLogLevel(level.toUpperCase())).toBe(level);
      expect(resolveLogLevel(`  ${level}  `)).toBe(level);
    }
  });

  it('falls back to the default for blank, missing, or non-string values', () => {
    expect(resolveLogLevel(undefined)).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel(null)).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel('')).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel('   ')).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel(42 as unknown as string)).toBe(DEFAULT_LOG_LEVEL);
  });

  it('rejects unknown levels instead of silently accepting them', () => {
    expect(resolveLogLevel('trace')).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel('LOUD')).toBe(DEFAULT_LOG_LEVEL);
    expect(resolveLogLevel('info-')).toBe(DEFAULT_LOG_LEVEL);
  });
});

describe('isSensitiveKey — classification', () => {
  it.each([
    'password',
    'passwd',
    'secret',
    'clientSecret',
    'token',
    'accessToken',
    'Authorization',
    'authorizationHeader',
    'apiKey',
    'api_key',
    'API-KEY',
    'privateKey',
    'secretKey',
    'jwt',
    'jwtToken',
    'credential',
    'credentials',
    'mnemonic',
    'seed',
    'seedPhrase',
    'cookie',
    'session',
    'signature',
  ])('treats %s as sensitive', (key) => {
    expect(isSensitiveKey(key)).toBe(true);
  });

  it.each(['author', 'publicKey', 'amount', 'userAddress', 'id', 'resourceId', 'message', 'level', 'path'])(
    'does not over-match %s',
    (key) => {
      expect(isSensitiveKey(key)).toBe(false);
    },
  );
});

describe('redactSensitive — success and boundary cases', () => {
  it('passes through primitives unchanged', () => {
    expect(redactSensitive('plain')).toBe('plain');
    expect(redactSensitive(1)).toBe(1);
    expect(redactSensitive(true)).toBe(true);
    expect(redactSensitive(null)).toBeNull();
    expect(redactSensitive(undefined)).toBeUndefined();
  });

  it('redacts sensitive keys at the top level and nested, preserving the rest', () => {
    const result = redactSensitive({
      username: 'alice',
      password: CANARY,
      amount: 100,
      wallet: { address: 'GABC', secretKey: CANARY },
    }) as Record<string, unknown>;

    expect(result.username).toBe('alice');
    expect(result.amount).toBe(100);
    expect(result.password).toBe(REDACTED);
    expect((result.wallet as Record<string, unknown>).address).toBe('GABC');
    expect((result.wallet as Record<string, unknown>).secretKey).toBe(REDACTED);
  });

  it('redacts sensitive keys inside arrays', () => {
    const result = redactSensitive({
      items: [{ token: CANARY }, { note: 'ok' }],
    }) as Record<string, unknown>;

    expect(result.items).toEqual([{ token: REDACTED }, { note: 'ok' }]);
  });

  it('does not throw on circular references and marks them', () => {
    const node: Record<string, unknown> = { name: 'root' };
    node.self = node;

    let result: Record<string, unknown> = {};
    expect(() => {
      result = redactSensitive(node) as Record<string, unknown>;
    }).not.toThrow();
    expect(result.self).toBe(CIRCULAR);
  });

  it('does not mark a shared (acyclic) reference as circular', () => {
    const shared = { value: 7 };
    const result = redactSensitive({ a: shared, b: shared }) as Record<string, unknown>;
    expect(result.a).toEqual({ value: 7 });
    expect(result.b).toEqual({ value: 7 });
  });

  it('bounds recursion depth', () => {
    let node: Record<string, unknown> = { leaf: true };
    for (let i = 0; i < 15; i += 1) {
      node = { [`level${i}`]: node };
    }

    const serialised = JSON.stringify(redactSensitive(node));
    expect(serialised).toContain(MAX_DEPTH_MARKER);
  });

  it('truncates oversized strings', () => {
    const oversized = 'x'.repeat(MAX_STRING_LENGTH + 500);
    const result = redactSensitive(oversized) as string;
    expect(result.length).toBeLessThan(oversized.length);
    expect(result.endsWith('...[truncated]')).toBe(true);
  });

  it('serialises BigInt, Symbol, function, and Date without throwing', () => {
    const result = redactSensitive({
      big: 10n,
      sym: Symbol('s'),
      fn: () => 'x',
      when: new Date('2026-01-01T00:00:00.000Z'),
    }) as Record<string, unknown>;

    expect(result.big).toBe('10');
    expect(result.sym).toBe('Symbol(s)');
    expect(typeof result.fn).toBe('string');
    expect(result.when).toBe('2026-01-01T00:00:00.000Z');
  });

  it('SECURITY: a sensitive canary value cannot appear anywhere in the output', () => {
    const payload = {
      user: { email: 'a@b.com', password: CANARY },
      auth: { apiKey: CANARY },
      history: [{ token: CANARY, nested: { credential: CANARY } }],
    };

    const serialised = JSON.stringify(redactSensitive(payload));
    expect(serialised).not.toContain(CANARY);
    expect(serialised).toContain(REDACTED);
  });

  it('SECURITY (teeth): the canary WOULD leak without redaction — proving the prior assertion is meaningful', () => {
    const payload = { password: CANARY };
    expect(JSON.stringify(payload)).toContain(CANARY);
  });
});

describe('serializeError', () => {
  it('keeps name, message, and stack for diagnosability', () => {
    const serialised = serializeError(new TypeError('bad input'));
    expect(serialised.name).toBe('TypeError');
    expect(serialised.message).toBe('bad input');
    expect(typeof serialised.stack).toBe('string');
  });

  it('includes enumerable custom fields but redacts sensitive ones', () => {
    const err = new Error('boom') as Error & { statusCode?: number; token?: string };
    err.statusCode = 503;
    err.token = CANARY;

    const serialised = serializeError(err);
    expect(serialised.statusCode).toBe(503);
    expect(serialised.token).toBe(REDACTED);
    expect(JSON.stringify(serialised)).not.toContain(CANARY);
  });
});

describe('buildLogger — integration and regression', () => {
  it('writes JSON records and redacts sensitive metadata before output', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'info', json: true, stream: capture.stream });

    logger.info('deposit', {
      userAddress: 'GABC',
      amount: 100,
      password: CANARY,
      wallet: { address: 'GABC', secretKey: CANARY },
    });

    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records).toHaveLength(1);
    expect(records[0].message).toBe('deposit');
    expect(records[0].userAddress).toBe('GABC');
    expect(records[0].amount).toBe(100);
    expect(records[0].password).toBe(REDACTED);
    const wallet = records[0].wallet as Record<string, unknown>;
    expect(wallet.address).toBe('GABC');
    expect(wallet.secretKey).toBe(REDACTED);
    expect(capture.text()).not.toContain(CANARY);
  });

  it('redacts an entire subtree when the container key is itself sensitive', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'info', json: true, stream: capture.stream });

    logger.info('auth event', { auth: { scheme: 'Bearer', apiKey: CANARY } });

    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records[0].auth).toBe(REDACTED);
    expect(capture.text()).not.toContain(CANARY);
  });

  it('redacts sensitive fields when an Error is attached as metadata', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'info', json: true, stream: capture.stream });

    logger.error('request failed', { err: new Error('boom'), token: CANARY });

    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records).toHaveLength(1);
    expect(records[0].token).toBe(REDACTED);
    const err = records[0].err as Record<string, unknown>;
    expect(err.message).toBe('boom');
    expect(capture.text()).not.toContain(CANARY);
  });

  it('survives circular and BigInt metadata without throwing', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'info', json: true, stream: capture.stream });

    const circular: Record<string, unknown> = { id: 1 };
    circular.self = circular;

    expect(() => logger.info('circular', { circular, big: 5n })).not.toThrow();

    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records).toHaveLength(1);
    expect((records[0].circular as Record<string, unknown>).self).toBe(CIRCULAR);
    expect((records[0].big as unknown)).toBe('5');
  });

  it('falls back to info for an invalid level and still emits records', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'not-a-level', json: true, stream: capture.stream });

    expect(logger.level).toBe('info');
    logger.info('still works');
    expect(capture.records()).toHaveLength(1);
  });

  it('honours a stricter valid level (error suppresses info)', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'error', json: true, stream: capture.stream });

    logger.info('should be suppressed');
    expect(capture.records()).toHaveLength(0);

    logger.error('should be emitted');
    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records).toHaveLength(1);
    expect(records[0].message).toBe('should be emitted');
  });

  it('keeps the compatibility contract: level/message are never redacted', () => {
    const capture = captureStream();
    const logger = buildLogger({ level: 'info', json: true, stream: capture.stream });

    logger.info('token is not a key here', { safe: true });

    const records = capture.records() as Array<Record<string, unknown>>;
    expect(records[0].level).toBe('info');
    expect(records[0].message).toBe('token is not a key here');
    expect(records[0].safe).toBe(true);
  });

  it('default export exposes the standard winston surface', () => {
    expect(defaultLogger).toBeInstanceOf(winston.Logger);
    for (const method of ['info', 'warn', 'error', 'debug', 'child'] as const) {
      expect(typeof (defaultLogger as unknown as Record<string, unknown>)[method]).toBe('function');
    }
  });
});
