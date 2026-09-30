/**
 * Cursor utilities for ledger-sequence-backed pagination
 */
export interface Cursor {
  ledgerSequence: number;
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

export function encodeCursor(ledgerSequence: number | Cursor, eventIndex?: number): string {
  let l = 0, e = 0;
  if (typeof ledgerSequence === 'object') {
    l = ledgerSequence.ledgerSequence;
    e = ledgerSequence.eventIndex;
  } else {
    l = ledgerSequence;
    e = eventIndex!;
  }
  const plain = `${l}${CURSOR_SEPARATOR}${e}`;
  return Buffer.from(plain, 'utf-8').toString('base64url');
}

export function decodeCursor(cursorString: string): Cursor {
  if (!cursorString || typeof cursorString !== 'string') {
    throw new CursorError('Cursor must be a non-empty string');
  }
  let plain = Buffer.from(cursorString, 'base64url').toString('utf-8');
  const parts = plain.split(CURSOR_SEPARATOR);
  if (parts.length !== 2) throw new CursorError('Invalid');
  const ledgerSequence = parseInt(parts[0], 10);
  const eventIndex = parseInt(parts[1], 10);
  if (isNaN(ledgerSequence) || isNaN(eventIndex)) throw new CursorError('Invalid');
  return { ledgerSequence, eventIndex };
}

export function sanitizePageSize(limit: unknown): number {
  if (limit === undefined || limit === null) return DEFAULT_PAGE_SIZE;
  const parsed = typeof limit === 'string' ? parseInt(limit, 10) : Number(limit);
  if (isNaN(parsed) || parsed < 1) return DEFAULT_PAGE_SIZE;
  return Math.min(parsed, MAX_PAGE_SIZE);
}

export function nextCursor(lastLedgerSequence: number, lastEventIndex: number): string {
  return encodeCursor({ ledgerSequence: lastLedgerSequence, eventIndex: lastEventIndex + 1 });
}

export function isValidCursor(value: unknown): value is string {
  if (typeof value !== 'string' || !value) return false;
  try { decodeCursor(value); return true; } catch { return false; }
}

export function getNextCursor<T extends { ledgerSequence: number; eventIndex: number }>(items: T[]): string | undefined {
  if (items.length === 0) return undefined;
  const last = items[items.length - 1];
  return encodeCursor(last.ledgerSequence, last.eventIndex);
}

export function compareCursors(a: string, b: string): number {
  const decodedA = decodeCursor(a);
  const decodedB = decodeCursor(b);
  if (decodedA.ledgerSequence !== decodedB.ledgerSequence) return decodedA.ledgerSequence - decodedB.ledgerSequence;
  return decodedA.eventIndex - decodedB.eventIndex;
}