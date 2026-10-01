/**
 * Cursor encoding/decoding utilities for ledger-sequence based pagination.
 *
 * Cursor format: base64(ledger_sequence:event_index)
 * Example: "MTAwMDow" decodes to "1000:0"
 *
 * This provides stable ordering guarantees even when new events arrive
 * between paginated requests.
 */

export interface Cursor {
  /** Ledger sequence number (monotonically increasing) */
  ledgerSequence: number;
  /** Event index within the ledger (0-based) */
  eventIndex: number;
}

const CURSOR_SEPARATOR = ':';

const MAX_LEDGER_SEQUENCE = 4_294_967_295;
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
 * Encodes a ledger sequence and event index into a cursor string.
 * Supports both (seq, idx) and ({ ledgerSequence, eventIndex }) forms.
 */
export function encodeCursor(
  cursorOrLedger: number | Cursor,
  eventIndex?: number
): string {
  let ledgerSeq: number;
  let evtIdx: number;

  if (typeof cursorOrLedger === 'object' && cursorOrLedger !== null) {
    ledgerSeq = cursorOrLedger.ledgerSequence;
    evtIdx = cursorOrLedger.eventIndex;
  } else {
    ledgerSeq = cursorOrLedger;
    evtIdx = eventIndex!;
  }

  if (!Number.isInteger(ledgerSeq) || ledgerSeq < 0) {
    throw new CursorError(`Invalid ledger sequence: ${ledgerSeq}`);
  }
  if (!Number.isInteger(evtIdx) || evtIdx < 0) {
    throw new CursorError(`Invalid event index: ${evtIdx}`);
  }

  const raw = `${ledgerSeq}${CURSOR_SEPARATOR}${evtIdx}`;
  return Buffer.from(raw, 'utf-8').toString('base64');
}

/**
 * Decodes a cursor string into ledger sequence and event index.
 */
export function decodeCursor(cursor: string): Cursor {
  if (!cursor || typeof cursor !== 'string') {
    throw new CursorError('Cursor decode failed: Cursor must be a non-empty string');
  }

  let decoded: string;
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

  const ledgerSequence = parseInt(parts[0], 10);
  const eventIndex = parseInt(parts[1], 10);

  if (isNaN(ledgerSequence) || isNaN(eventIndex) || !/^-?\d+$/.test(parts[0]) || !/^-?\d+$/.test(parts[1])) {
    throw new CursorError('Invalid cursor: ledger sequence and event index must be integers');
  }

  if (ledgerSequence < 0 || eventIndex < 0) {
    throw new CursorError('Invalid cursor: values must be non-negative');
  }

  return { ledgerSequence, eventIndex };
}

/**
 * Checks if a cursor is valid without throwing.
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
 */
export function getNextCursor<T extends { ledgerSequence: number; eventIndex: number }>(
  items: T[]
): string | undefined {
  if (!items || items.length === 0) return undefined;
  const last = items[items.length - 1];
  return encodeCursor(last.ledgerSequence, last.eventIndex);
}

/**
 * Generate the next cursor from the last item in a result set
 */
export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({
    ledgerSequence: lastLedgerSequence,
    eventIndex: lastEventIndex + 1,
  });
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
}

/**
 * Validate and sanitize page size parameter
 */
export function sanitizePageSize(limit: unknown): number {
  if (limit === undefined || limit === null) {
    return DEFAULT_PAGE_SIZE;
  }

  const parsed = typeof limit === 'string' ? parseInt(limit, 10) : Number(limit);

  if (isNaN(parsed) || parsed < 1) {
    return DEFAULT_PAGE_SIZE;
  }

  return Math.min(parsed, MAX_PAGE_SIZE);
}