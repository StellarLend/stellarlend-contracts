import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { calculateJitterDelay } from '../src/services/contract-updater';

describe('Oracle ContractUpdater Backoff & Jitter Distribution', () => {
  beforeEach(() => {
    vi.spyOn(Math, 'random');
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('should correctly calculate exponential progression before hitting the cap', () => {
    vi.mocked(Math.random).mockReturnValue(1.0);
    const base = 1000;
    const cap = 10000;

    expect(calculateJitterDelay(0, base, cap)).toBe(1000);
    expect(calculateJitterDelay(1, base, cap)).toBe(2000);
    expect(calculateJitterDelay(2, base, cap)).toBe(4000);
  });

  it('should cap out gracefully at the backoffCapMs boundary', () => {
    vi.mocked(Math.random).mockReturnValue(1.0);
    const base = 1000;
    const cap = 10000;

    expect(calculateJitterDelay(4, base, cap)).toBe(10000);
  });

  it('should apply full jitter variance between 0 and the max limit', () => {
    vi.mocked(Math.random).mockReturnValue(0.5);
    const base = 1000;
    const cap = 10000;

    expect(calculateJitterDelay(1, base, cap)).toBe(1000);
  });
});
