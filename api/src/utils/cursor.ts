/**
 * Cursor encoding/decoding utilities for ledger-sequence based pagination.
 * 
 * Cursor format: base64(ledger_sequence:event_index)
 * Example: "MTAwMDow" decodes to "1000:0"
 * 
 * This provides stable ordering guarantees even when new events arrive
 * between paginated requests.
 */

const CURSOR_SEPARATOR = ':';

/**
 * Encodes a ledger sequence and event index into a cursor string.
 */
export function encodeCursor(ledgerSequence: number, eventIndex: number): string {
  if (!Number.isInteger(ledgerSequence) || ledgerSequence < 0) {
    throw new Error(`Invalid ledger sequence: ${ledgerSequence}`);
  }
  if (!Number.isInteger(eventIndex) || eventIndex < 0) {
    throw new Error(`Invalid event index: ${eventIndex}`);
  }
  
  const raw = `${ledgerSequence}${CURSOR_SEPARATOR}${eventIndex}`;
  return Buffer.from(raw, 'utf-8').toString('base64');
}

/**
 * Decodes a cursor string into ledger sequence and event index.
 */
export function decodeCursor(cursor: string): { ledgerSequence: number; eventIndex: number } {
  try {
    const decoded = Buffer.from(cursor, 'base64').toString('utf-8');
    const parts = decoded.split(CURSOR_SEPARATOR);
    
    if (parts.length !== 2) {
      throw new Error('Invalid cursor format: expected "ledger_sequence:event_index"');
    }
    
    const ledgerSequence = parseInt(parts[0], 10);
    const eventIndex = parseInt(parts[1], 10);
    
    if (isNaN(ledgerSequence) || isNaN(eventIndex)) {
      throw new Error('Invalid cursor: ledger sequence and event index must be integers');
    }
    
    if (ledgerSequence < 0 || eventIndex < 0) {
      throw new Error('Invalid cursor: values must be non-negative');
    }
    
    return { ledgerSequence, eventIndex };
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`Cursor decode failed: ${error.message}`);
    }
    throw new Error('Cursor decode failed: unknown error');
 * Cursor utilities for ledger-sequence-backed pagination
 * 
 * Cursor format: base64(ledger_sequence:event_index)
 * 
 * This provides stable ordering guarantees even when new events
 * arrive between paginated API calls. The cursor is opaque to clients
 * and encodes both the ledger sequence and event index within that ledger.
 * 
 * @see docs/ACTIVITY_ORDERING_GUARANTEES.md
 */

export interface Cursor {
  /** Ledger sequence number (monotonically increasing) */
  ledgerSequence: number;
  /** Event index within the ledger (0-based) */
  eventIndex: number;
}

/** Separator between ledger sequence and event index in cursor string */
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
 * Encode a cursor object to an opaque base64 string
 * 
 * @param cursor - The cursor to encode
 * @returns Base64-encoded cursor string
 */
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

/**
 * Decode a base64 cursor string back to a Cursor object
 * 
 * @param cursorString - The base64-encoded cursor
 * @returns Parsed cursor object
 * @throws CursorError if the cursor is malformed or out of range
 */
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

/**
 * Validate and sanitize page size parameter
 * 
 * @param limit - Raw limit from query parameter
 * @returns Sanitized limit between 1 and MAX_PAGE_SIZE
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

/**
 * Generate the next cursor from the last item in a result set
 * 
 * @param lastLedgerSequence - Ledger sequence of the last item
 * @param lastEventIndex - Event index of the last item
 * @returns Encoded cursor for the next page
 */
export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({
    ledgerSequence: lastLedgerSequence,
    eventIndex: lastEventIndex + 1,
  });
}

/**
 * Custom error class for cursor operations
 */
export class CursorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CursorError';
  }
}

/**
 * Checks if a cursor is valid without throwing.
 */
export function isValidCursor(cursor: string): boolean {
  try {
    decodeCursor(cursor);
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
 */
export function getNextCursor<T extends { ledgerSequence: number; eventIndex: number }>(
  items: T[]
): string | undefined {
  if (items.length === 0) return undefined;
  const last = items[items.length - 1];
  return encodeCursor(last.ledgerSequence, last.eventIndex);
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