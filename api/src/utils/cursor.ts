/**
 * Cursor encoding/decoding utilities for ledger-sequence based pagination.
 *
 * Cursor format: base64url(ledger_sequence:event_index)
 * Example: "MTAwMDow" decodes to "1000:0"
 *
 * This provides stable ordering guarantees even when new events arrive
 * between paginated requests.
 *
 * @see docs/ACTIVITY_ORDERING_GUARANTEES.md
 * Cursor utilities for ledger-sequence-backed pagination.
 *
 * Cursor format: base64url(ledger_sequence:event_index)
 *
 * Provides stable ordering guarantees even when new events arrive between
 * paginated API calls. The cursor is opaque to callers.
 */

/** A cursor position within the ledger stream. */
export interface Cursor {
  ledgerSequence: number;
  eventIndex: number;
}

const CURSOR_SEPARATOR = ':';

/**
 * Maximum supported ledger sequence (u32 max)
 * Prevents integer overflow in parsing
 */
const MAX_LEDGER_SEQUENCE = 4_294_967_295;

/**
 * Maximum supported event index per ledger
 * Prevents unbounded memory allocation attacks
 */
const MAX_EVENT_INDEX = 1_000_000;

/**
 * Default page size for activity queries
 */
export const DEFAULT_PAGE_SIZE = 20;

/**
 * Maximum page size to prevent DoS
 */
export const MAX_PAGE_SIZE = 100;

/**
 * Custom error class for cursor operations
 */
// u32 max — Stellar ledger sequences are unsigned 32-bit integers
const MAX_LEDGER_SEQUENCE = 4_294_967_295;
// Practical cap to prevent unbounded parsing
const MAX_EVENT_INDEX = 1_000_000;

export const DEFAULT_PAGE_SIZE = 20;
export const MAX_PAGE_SIZE = 100;

export class CursorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CursorError';
  }
}

/**
 * Encode a cursor object to an opaque base64url string
 *
 * @param cursor - The cursor to encode
 * @returns Base64url-encoded cursor string
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
 * Decode a base64url cursor string back to a Cursor object
 *
 * @param cursorString - The base64url-encoded cursor
 * @returns Parsed cursor object
 * @throws CursorError if the cursor is malformed or out of range
 */
export function decodeCursor(cursorString: string): Cursor {
  if (!cursorString || typeof cursorString !== 'string') {
    throw new CursorError('Cursor must be a non-empty string');
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
 * Validate and sanitize page size parameter
 *
 * @param limit - Raw limit from query parameter
 * @returns Sanitized limit between 1 and MAX_PAGE_SIZE
 */
export function sanitizePageSize(limit: unknown): number {
  if (limit === undefined || limit === null) {
    return DEFAULT_PAGE_SIZE;
export function isValidCursor(value: unknown): value is string {
  if (typeof value !== 'string' || !value) return false;
  try {
    decodeCursor(value);
    return true;
  } catch {
    return false;
  }
}

export function sanitizePageSize(limit: unknown): number {
  if (limit === undefined || limit === null) return DEFAULT_PAGE_SIZE;

  const parsed = typeof limit === 'string' ? parseInt(limit, 10) : Number(limit);
  if (isNaN(parsed) || parsed < 1) return DEFAULT_PAGE_SIZE;

  return Math.min(parsed, MAX_PAGE_SIZE);
}

/**
 * Generate the next cursor from the last item in a result set
 *
 * @param lastLedgerSequence - Ledger sequence of the last item
 * @param lastEventIndex - Event index of the last item
 * @returns Encoded cursor for the next page
 * Builds the cursor that points to the position *after* the given item,
 * i.e. the start position for the next page.
 */
export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({
    ledgerSequence: lastLedgerSequence,
    eventIndex: lastEventIndex + 1,
  });
}

/**
 * Check if a value is a valid cursor string
 *
 * @param value - Value to check
 * @returns true if valid cursor, false otherwise
 */
export function isValidCursor(value: unknown): value is string {
  if (typeof value !== 'string' || !value) {
    return false;
  }
  try {
    decodeCursor(value);
    return true;
  } catch {
    return false;
  }
}

/**
 * Extracts the next cursor from the last item in a result set.
 * Kept for backward compatibility with existing callers.
// ---------------------------------------------------------------------------
// Legacy helpers — preserved for backwards compatibility
// ---------------------------------------------------------------------------

/**
 * @deprecated Use encodeCursor({ ledgerSequence, eventIndex }) instead.
 */
export function getNextCursor<T extends { ledgerSequence: number; eventIndex: number }>(
  items: T[]
): string | undefined {
  if (items.length === 0) return undefined;
  const last = items[items.length - 1];
  return encodeCursor({ ledgerSequence: last.ledgerSequence, eventIndex: last.eventIndex });
}

/**
 * Compares two cursors for ordering.
 * Returns negative if a < b, positive if a > b, 0 if equal.
 */
export function compareCursors(a: string, b: string): number {
  const decodedA = decodeCursor(a);
  const decodedB = decodeCursor(b);

  if (decodedA.ledgerSequence !== decodedB.ledgerSequence) {
    return decodedA.ledgerSequence - decodedB.ledgerSequence;
  }
  return decodedA.eventIndex - decodedB.eventIndex;
  const da = decodeCursor(a);
  const db = decodeCursor(b);
  if (da.ledgerSequence !== db.ledgerSequence) return da.ledgerSequence - db.ledgerSequence;
  return da.eventIndex - db.eventIndex;
}
