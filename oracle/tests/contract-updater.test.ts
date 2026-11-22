import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ContractUpdater, calculateJitterDelay } from '../src/services/contract-updater';

describe('Oracle ContractUpdater Backoff & Jitter Distribution', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps the deterministic jitter result within the exponential bound', () => {
    const base = 1000;
    const cap = 10000;

    expect(calculateJitterDelay(0, base, cap)).toBeGreaterThanOrEqual(0);
    expect(calculateJitterDelay(0, base, cap)).toBeLessThanOrEqual(base);
    expect(calculateJitterDelay(1, base, cap)).toBeLessThanOrEqual(base * 2);
    expect(calculateJitterDelay(2, base, cap)).toBeLessThanOrEqual(base * 4);
  });

  it('does not exceed the cap after the exponential bound saturates', () => {
    const base = 1000;
    const cap = 2000;

    expect(calculateJitterDelay(4, base, cap)).toBeGreaterThanOrEqual(0);
    expect(calculateJitterDelay(4, base, cap)).toBeLessThanOrEqual(cap);
    expect(calculateJitterDelay(10, base, cap)).toBeLessThanOrEqual(cap);
  });

  it('produces a stable result for the same attempt and bounds', () => {
    const base = 1000;
    const cap = 10000;

    expect(calculateJitterDelay(1, base, cap)).toBe(calculateJitterDelay(1, base, cap));
  });
});

describe('ContractUpdater failure paths and boundaries', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  const request = (overrides = {}) => ({
    asset: 'XLM',
    price: 100,
    source: 'test',
    observedAt: Date.now(),
    idempotencyKey: 'update-1',
    ...overrides,
  });

  const aggregatedPrice = (asset: string, price: bigint) => ({
    asset,
    price,
    timestamp: Math.floor(Date.now() / 1000),
    confidence: 95,
    sources: [],
  });

  it('updatePrice_never_throws', async () => {
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn().mockRejectedValue(
        Object.assign(new Error('submission rejected'), {
          code: 'INVALID_ASSET',
        }),
      ),
    };
    const updater = new ContractUpdater(adapter);

    await expect(
      updater.updatePrice(aggregatedPrice('XLM', 100n)),
    ).resolves.toMatchObject({
      success: false,
      asset: 'XLM',
      error: 'submission rejected',
    });
  });

  it('updatePrices_continues_after_one_asset_fails', async () => {
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn()
        .mockRejectedValueOnce(
          Object.assign(new Error('first asset rejected'), {
            code: 'INVALID_ASSET',
          }),
        )
        .mockResolvedValueOnce({ txHash: 'tx-second' }),
    };
    const updater = new ContractUpdater(adapter);

    const results = await updater.updatePrices([
      aggregatedPrice('FAIL', 100n),
      aggregatedPrice('BTC', 200n),
    ]);

    expect(results.map(({ success }) => success)).toEqual([false, true]);
    expect(results[0].error).toBe('first asset rejected');
    expect(results[1].transactionHash).toBe('tx-second');
    expect(adapter.submit).toHaveBeenCalledTimes(2);
  });

  it('rejects stale and future requests before touching the adapter', async () => {
    const adapter = { getLatestUpdate: vi.fn(), submit: vi.fn() };
    const updater = new ContractUpdater(adapter);

    await expect(updater.submitPriceUpdate(request({ observedAt: Date.now() - 300001 }))).rejects.toThrow('stale');
    await expect(updater.submitPriceUpdate(request({ observedAt: Date.now() + 5001 }))).rejects.toThrow('stale');
    expect(adapter.getLatestUpdate).not.toHaveBeenCalled();
    expect(adapter.submit).not.toHaveBeenCalled();
  });

  it('rejects non-retryable adapter errors without retrying', async () => {
    const error = Object.assign(new Error('asset is invalid'), { code: 'INVALID_ASSET' });
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn().mockRejectedValue(error),
    };
    const updater = new ContractUpdater(adapter);

    await expect(updater.submitPriceUpdate(request())).rejects.toThrow('asset is invalid');
    expect(adapter.submit).toHaveBeenCalledTimes(1);
    expect(updater.subs.get('update-1')).toMatchObject({ status: 'REJECTED', attempts: 1, error: 'asset is invalid' });
    expect(updater.latest.has('XLM')).toBe(false);
  });

  it('recovers from a retryable failure and confirms on the next attempt', async () => {
    vi.useFakeTimers();
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn()
        .mockRejectedValueOnce(new Error('temporary outage'))
        .mockResolvedValueOnce({ txHash: 'tx-2' }),
    };
    const updater = new ContractUpdater(adapter);

    const update = updater.submitPriceUpdate(request());
    await vi.runAllTimersAsync();
    await expect(update).resolves.toBeUndefined();

    expect(adapter.submit).toHaveBeenCalledTimes(2);
    expect(updater.subs.get('update-1')).toMatchObject({ status: 'CONFIRMED', attempts: 2, txHash: 'tx-2' });
    expect(updater.latest.get('XLM')).toBe(updater.subs.get('update-1'));
  });

  it('does not submit a duplicate confirmed price', async () => {
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn().mockResolvedValue({ txHash: 'tx-1' }),
    };
    const updater = new ContractUpdater(adapter);

    await updater.submitPriceUpdate(request());
    await updater.submitPriceUpdate(request({ idempotencyKey: 'update-2' }));

    expect(adapter.submit).toHaveBeenCalledTimes(1);
  });

  it('serializes updates for the same asset', async () => {
    let releaseFirst: (() => void) | undefined;
    const firstComplete = new Promise<void>((resolve) => { releaseFirst = resolve; });
    const order: string[] = [];
    const adapter = {
      getLatestUpdate: vi.fn().mockResolvedValue(null),
      submit: vi.fn().mockImplementation(async (submission) => {
        order.push(submission.id);
        if (submission.id === 'update-1') await firstComplete;
        return { txHash: submission.id };
      }),
    };
    const updater = new ContractUpdater(adapter);

    const first = updater.submitPriceUpdate(request());
    const second = updater.submitPriceUpdate(request({ idempotencyKey: 'update-2', price: 101 }));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(order).toEqual(['update-1']);

    releaseFirst?.();
    await Promise.all([first, second]);
    expect(order).toEqual(['update-1', 'update-2']);
  });
});
