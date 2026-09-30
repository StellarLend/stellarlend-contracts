/**
 * Cursor utilities for ledger-sequence-backed pagination.
 *
 * Cursor format: base64url(ledger_sequence:event_index)
 *
 * Provides stable ordering guarantees even when new events arrive between
 * paginated API calls. The cursor is opaque to callers.
 */

export interface Cursor {
  ledgerSequence: number;
  eventIndex: number;
}

const CURSOR_SEPARATOR = ':';

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

export function encodeCursor(cursor: Cursor): string {
  if (cursor.ledgerSequence < 0 || cursor.ledgerSequence > MAX_LEDGER_SEQUENCE) {
    throw new CursorError(`Invalid ledger sequence: ${cursor.ledgerSequence}`);
  }
  if (cursor.eventIndex < 0 || cursor.eventIndex > MAX_EVENT_INDEX) {
    throw new CursorError(`Invalid event index: ${cursor.eventIndex}`);
  }

  const plain = `${cursor.ledgerSequence}${CURSOR_SEPARATOR}${cursor.eventIndex}`;
  return Buffer.from(plain, 'utf-8').toString('base64url');
}

export function decodeCursor(cursorString: string): Cursor {
  if (!cursorString || typeof cursorString !== 'string') {
    throw new CursorError('Cursor must be a non-empty string');
  }

  let plain: string;
  try {
    plain = Buffer.from(cursorString, 'base64url').toString('utf-8');
  } catch {
    throw new CursorError('Invalid base64 encoding');
  }

  const parts = plain.split(CURSOR_SEPARATOR);
  if (parts.length !== 2) {
    throw new CursorError(`Invalid cursor format: expected "ledger:event", got "${plain}"`);
  }

  const ledgerSequence = parseInt(parts[0], 10);
  const eventIndex = parseInt(parts[1], 10);

  if (isNaN(ledgerSequence) || isNaN(eventIndex)) {
    throw new CursorError('Cursor contains non-numeric values');
  }

  if (ledgerSequence < 0 || ledgerSequence > MAX_LEDGER_SEQUENCE) {
    throw new CursorError(`Ledger sequence out of range: ${ledgerSequence}`);
  }
  if (eventIndex < 0 || eventIndex > MAX_EVENT_INDEX) {
    throw new CursorError(`Event index out of range: ${eventIndex}`);
  }

  return { ledgerSequence, eventIndex };
}

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
 * Builds the cursor that points to the position *after* the given item,
 * i.e. the start position for the next page.
 */
export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({
    ledgerSequence: lastLedgerSequence,
    eventIndex: lastEventIndex + 1,
  });
}

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
  const da = decodeCursor(a);
  const db = decodeCursor(b);
  if (da.ledgerSequence !== db.ledgerSequence) return da.ledgerSequence - db.ledgerSequence;
  return da.eventIndex - db.eventIndex;
}
