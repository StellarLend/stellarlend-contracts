/**
 * Cursor utilities for ledger-sequence-backed pagination.
 *
 * Cursor format: base64url(ledger_sequence:event_index)
 *
 * This provides stable ordering guarantees even when new events arrive
 * between paginated API calls. The cursor is opaque to clients and encodes
 * both the ledger sequence and the event index within that ledger.
 *
 * Canonical contract (see StellarService.fetchActivityByLedgerRange, which
 * consumes the object form).
 */

/** A cursor position within the ledger stream. */
export interface Cursor {
  ledgerSequence: number;
  eventIndex: number;
}

const CURSOR_SEPARATOR = ':';

/**
 * Maximum supported ledger sequence (u32 max).
 * Prevents integer overflow in parsing.
 */
const MAX_LEDGER_SEQUENCE = 4_294_967_295;

/**
 * Maximum supported event index per ledger.
 * Prevents unbounded memory allocation attacks.
 */
const MAX_EVENT_INDEX = 1_000_000;

/** Default page size for activity queries */
export const DEFAULT_PAGE_SIZE = 20;

/** Maximum page size to prevent DoS */
export const MAX_PAGE_SIZE = 100;

export class CursorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CursorError';
  }
}

/**
 * Custom error class for cursor operations.
 */
export class CursorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CursorError';
    Object.setPrototypeOf(this, CursorError.prototype);
  }
}

/**
 * Encode a cursor object to an opaque base64url string.
 */
export function encodeCursor(cursor: Cursor): string {
  const { ledgerSequence, eventIndex } = cursor ?? ({} as Cursor);

  if (!Number.isInteger(ledgerSequence) || ledgerSequence < 0 || ledgerSequence > MAX_LEDGER_SEQUENCE) {
    throw new CursorError(`Invalid ledger sequence: ${ledgerSequence}`);
  }
  if (!Number.isInteger(eventIndex) || eventIndex < 0 || eventIndex > MAX_EVENT_INDEX) {
    throw new CursorError(`Invalid event index: ${eventIndex}`);
  }

  const plain = `${ledgerSequence}${CURSOR_SEPARATOR}${eventIndex}`;
  return Buffer.from(plain, 'utf-8').toString('base64url');
}

/**
 * Decode a base64url cursor string back to a Cursor object.
 *
 * @throws CursorError if the cursor is malformed or out of range.
 */
export function decodeCursor(cursor: string): Cursor {
  if (!cursor || typeof cursor !== 'string') {
    throw new CursorError('Cursor decode failed: Cursor must be a non-empty string');
  }

  // base64url and base64 decode identically for the alphabet we emit, but be
  // defensive: a cursor that is not valid base64 must not silently decode to
  // garbage.
  if (!/^[A-Za-z0-9_-]+={0,2}$/.test(cursorString)) {
    throw new CursorError('Invalid base64 encoding');
  }

  let plain: string;
  try {
    const buf = Buffer.from(cursor, 'base64');
    // Basic validation of base64 characters
    if (/[^A-Za-z0-9+/=_-]/.test(cursor)) {
      throw new Error('Invalid base64 characters');
    }
    decoded = buf.toString('utf-8');
  } catch (error) {
    throw new CursorError(`Cursor decode failed: ${(error as Error).message}`);
  }

  const parts = decoded.split(CURSOR_SEPARATOR);
  if (parts.length !== 2) {
    throw new CursorError('Invalid cursor format: expected "ledger_sequence:event_index"');
  }

  if (!/^\d+$/.test(parts[0]) || !/^\d+$/.test(parts[1])) {
    throw new CursorError('Cursor contains non-numeric values');
  }

  const ledgerSequence = Number(parts[0]);
  const eventIndex = Number(parts[1]);

  if (!Number.isSafeInteger(ledgerSequence) || !Number.isSafeInteger(eventIndex)) {
    throw new CursorError('Cursor contains non-numeric values');
  }
  if (ledgerSequence > MAX_LEDGER_SEQUENCE) {
    throw new CursorError(`Ledger sequence out of range: ${ledgerSequence}`);
  }
  if (eventIndex > MAX_EVENT_INDEX) {
    throw new CursorError(`Event index out of range: ${eventIndex}`);
  }

  return { ledgerSequence, eventIndex };
}

/**
 * Validate and sanitize a page size parameter.
 *
 * Non-numeric, zero and negative values fall back to the default so that
 * hostile query strings cannot force unbounded result sets.
 */
export function sanitizePageSize(limit: unknown): number {
  if (limit === undefined || limit === null) {
    return DEFAULT_PAGE_SIZE;
  }

  const parsed = typeof limit === 'string' ? parseInt(limit, 10) : Number(limit);

  if (!Number.isFinite(parsed) || Number.isNaN(parsed) || parsed < 1) {
    return DEFAULT_PAGE_SIZE;
  }

  return Math.min(Math.trunc(parsed), MAX_PAGE_SIZE);
}

/**
 * Generate the cursor pointing at the position immediately after the last
 * item in a page.
 */
export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({
    ledgerSequence: lastLedgerSequence,
    eventIndex: lastEventIndex + 1,
  });
}

/**
 * Check if a value is a valid cursor string.
 */
export function isValidCursor(value: unknown): value is string {
  if (typeof value !== 'string' || !value) return false;
  try { decodeCursor(value); return true; } catch { return false; }
}

/**
 * Compare two cursors for ordering.
 * Returns negative if a < b, positive if a > b, 0 if equal.
 */
export function compareCursors(a: string, b: string): number {
  const decodedA = decodeCursor(a);
  const decodedB = decodeCursor(b);

  if (decodedA.ledgerSequence !== decodedB.ledgerSequence) {
    return decodedA.ledgerSequence - decodedB.ledgerSequence;
  }
  return decodedA.eventIndex - decodedB.eventIndex;
}
