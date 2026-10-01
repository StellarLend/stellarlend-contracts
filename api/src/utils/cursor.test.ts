import {
  encodeCursor,
  decodeCursor,
  nextCursor,
  sanitizePageSize,
  isValidCursor,
  compareCursors,
  CursorError,
  DEFAULT_PAGE_SIZE,
  MAX_PAGE_SIZE,
} from './cursor';

describe('cursor utilities', () => {
  describe('encodeCursor', () => {
    it('encodes valid ledger sequence and event index', () => {
      const cursor = encodeCursor({ ledgerSequence: 1000, eventIndex: 5 });
      expect(cursor).toBe('MTAwMDo1');
      expect(Buffer.from(cursor, 'base64url').toString('utf-8')).toBe('1000:5');
    });

    it('encodes zero values', () => {
      const cursor = encodeCursor({ ledgerSequence: 0, eventIndex: 0 });
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 0, eventIndex: 0 });
    });

    it('encodes large values within bounds', () => {
      const cursor = encodeCursor({ ledgerSequence: 4_294_967_295, eventIndex: 1_000_000 });
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 4_294_967_295, eventIndex: 1_000_000 });
    });

    it('produces url-safe base64 without padding', () => {
      const cursor = encodeCursor({ ledgerSequence: 1, eventIndex: 0 });
      expect(cursor).not.toContain('=');
      expect(cursor).not.toContain('+');
      expect(cursor).not.toContain('/');
    });

    it('throws on negative ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(
        'Invalid ledger sequence: -1'
      );
    });

    it('throws on negative event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
    });

    it('throws on non-integer ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: 1.5, eventIndex: 0 })).toThrow(CursorError);
    });

    it('throws on non-integer event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: 1.5 })).toThrow(CursorError);
    });

    it('throws when ledger sequence exceeds u32 max', () => {
      expect(() => encodeCursor({ ledgerSequence: 4_294_967_296, eventIndex: 0 })).toThrow(
        CursorError
      );
    });

    it('throws when event index exceeds the per-ledger maximum', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: 1_000_001 })).toThrow(CursorError);
    });
  });

  describe('decodeCursor', () => {
    it('decodes valid cursor', () => {
      const encoded = encodeCursor({ ledgerSequence: 1000, eventIndex: 5 });
      expect(decodeCursor(encoded)).toEqual({ ledgerSequence: 1000, eventIndex: 5 });
    });

    it('throws on invalid base64', () => {
      expect(() => decodeCursor('not-valid-base64!!!')).toThrow(CursorError);
      expect(() => decodeCursor('not-valid-base64!!!')).toThrow('Invalid base64 encoding');
    });

    it('throws on missing separator', () => {
      const bad = Buffer.from('1000', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on too many separators', () => {
      const bad = Buffer.from('1000:5:extra', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on non-numeric values', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('throws on negative values in decoded cursor', () => {
      // Negative values do not match the digits-only pattern, so they are
      // rejected as non-numeric rather than range-checked.
      const bad = Buffer.from('-1:-1', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('throws on empty string', () => {
      expect(() => decodeCursor('')).toThrow(CursorError);
    });

    it('throws on null and undefined', () => {
      expect(() => decodeCursor(null as unknown as string)).toThrow(CursorError);
      expect(() => decodeCursor(undefined as unknown as string)).toThrow(CursorError);
    });

    it('throws when ledger sequence is out of range', () => {
      const bad = Buffer.from('99999999999:0', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Ledger sequence out of range');
    });

    it('throws when event index is out of range', () => {
      const bad = Buffer.from('1000:9999999', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Event index out of range');
    });
  });

  describe('nextCursor', () => {
    it('returns the position after the given event', () => {
      const cursor = nextCursor(1000, 5);
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 1000, eventIndex: 6 });
    });

    it('handles rollover to the maximum event index', () => {
      const cursor = nextCursor(1000, 999_999);
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 1000, eventIndex: 1_000_000 });
    });
  });

  describe('sanitizePageSize', () => {
    it('returns the default when limit is absent', () => {
      expect(sanitizePageSize(undefined)).toBe(DEFAULT_PAGE_SIZE);
      expect(sanitizePageSize(null)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('parses numeric strings', () => {
      expect(sanitizePageSize('50')).toBe(50);
    });

    it('caps at MAX_PAGE_SIZE', () => {
      expect(sanitizePageSize(200)).toBe(MAX_PAGE_SIZE);
      expect(sanitizePageSize('200')).toBe(MAX_PAGE_SIZE);
    });

    it('falls back to the default for non-numeric input', () => {
      expect(sanitizePageSize('abc')).toBe(DEFAULT_PAGE_SIZE);
      expect(sanitizePageSize({})).toBe(DEFAULT_PAGE_SIZE);
      expect(sanitizePageSize([])).toBe(DEFAULT_PAGE_SIZE);
    });

    it('falls back to the default for zero and negative input', () => {
      expect(sanitizePageSize(0)).toBe(DEFAULT_PAGE_SIZE);
      expect(sanitizePageSize(-5)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('does not allow a fractional or hostile value to shrink the cap', () => {
      expect(sanitizePageSize(1.9)).toBe(1);
      expect(sanitizePageSize(Number.MAX_SAFE_INTEGER)).toBe(MAX_PAGE_SIZE);
      expect(sanitizePageSize(Number.POSITIVE_INFINITY)).toBe(DEFAULT_PAGE_SIZE);
      expect(sanitizePageSize(Number.NaN)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('accepts valid page sizes', () => {
      expect(sanitizePageSize(1)).toBe(1);
      expect(sanitizePageSize(50)).toBe(50);
      expect(sanitizePageSize(MAX_PAGE_SIZE)).toBe(MAX_PAGE_SIZE);
    });
  });

  describe('isValidCursor', () => {
    it('returns true for a valid cursor', () => {
      expect(isValidCursor(encodeCursor({ ledgerSequence: 100, eventIndex: 0 }))).toBe(true);
    });

    it('returns false for invalid cursors', () => {
      expect(isValidCursor('garbage')).toBe(false);
      expect(isValidCursor('')).toBe(false);
    });

    it('returns false for non-string input', () => {
      expect(isValidCursor(123)).toBe(false);
      expect(isValidCursor(null)).toBe(false);
      expect(isValidCursor(undefined)).toBe(false);
    });
  });

  describe('compareCursors', () => {
    it('returns negative when a < b (ledger)', () => {
      const a = encodeCursor({ ledgerSequence: 100, eventIndex: 0 });
      const b = encodeCursor({ ledgerSequence: 200, eventIndex: 0 });
      expect(compareCursors(a, b)).toBeLessThan(0);
    });

    it('returns positive when a > b (ledger)', () => {
      const a = encodeCursor({ ledgerSequence: 200, eventIndex: 0 });
      const b = encodeCursor({ ledgerSequence: 100, eventIndex: 0 });
      expect(compareCursors(a, b)).toBeGreaterThan(0);
    });

    it('compares by event index when ledger is equal', () => {
      const a = encodeCursor({ ledgerSequence: 100, eventIndex: 0 });
      const b = encodeCursor({ ledgerSequence: 100, eventIndex: 5 });
      expect(compareCursors(a, b)).toBeLessThan(0);
    });

    it('returns 0 when equal', () => {
      const a = encodeCursor({ ledgerSequence: 100, eventIndex: 5 });
      const b = encodeCursor({ ledgerSequence: 100, eventIndex: 5 });
      expect(compareCursors(a, b)).toBe(0);
    });
  });
});

describe('Cursor numeric bounds', () => {
  it('rejects a ledger sequence that cannot be represented exactly', () => {
    const oversized = Buffer.from('99999999999999999999:1').toString('base64url');

    expect(() => decodeCursor(oversized)).toThrow(CursorError);
  });

  it('rejects an event index that cannot be represented exactly', () => {
    const oversized = Buffer.from('1:99999999999999999999').toString('base64url');

    expect(() => decodeCursor(oversized)).toThrow(CursorError);
  });
});
