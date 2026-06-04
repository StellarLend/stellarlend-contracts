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
  }
}

/**
 * Checks if a cursor is valid without throwing.
 */
export function isValidCursor(cursor: string): boolean {
  try {
    decodeCursor(cursor);
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