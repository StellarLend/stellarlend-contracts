import { createHash } from 'crypto';

/**
 * Contract Updater — deterministic, failure-safe price upload orchestration.
 *
 * Invariants (enforced below):
 *  1. Per-asset serialization: updates for the same asset never run concurrently.
 *  2. Idempotency: a submission with a known idempotency key in a non-terminal
 *     or successful state is a no-op; only FAILED/REJECTED/CANCELLED may be retried.
 *  3. Staleness: observedAt must be within [now - STALE_MS, now + FUTURE_SKEW_MS].
 *  4. Retry bounds: attempts are capped by MAX_RETRIES and by expiresAt (TIMEOUT_MS).
 *  5. Non-retryable errors (PRICE_STALE, INVALID_ASSET, SOURCE_UNAVAILABLE)
 *     terminate immediately as REJECTED without burning retries.
 *  6. State transitions are terminal-only: PENDING → CONFIRMED/FAILED/REJECTED/CANCELLED.
 *  7. Failures never leave a stale CONFIRMED entry in the latest map.
 */

export const MAX_RETRIES = 3;
export const BASE_MS = 250;
export const CAP_MS = 8000;
export const TIMEOUT_MS = 120000;
export const STALE_MS = 300000;
export const FUTURE_SKEW_MS = 5000;

const TERMINAL_STATUSES = ['FAILED', 'REJECTED', 'CANCELLED'];
const NON_RETRYABLE_CODES = ['PRICE_STALE', 'INVALID_ASSET', 'SOURCE_UNAVAILABLE'];

export function calculateJitterDelay(attempt, base = BASE_MS, cap = CAP_MS) {
  const safeAttempt = Number.isFinite(attempt) && attempt >= 0 ? Math.floor(attempt) : 0;
  const capped = Math.min(cap, base * Math.pow(2, safeAttempt));
  const ratio = (((safeAttempt + 1) * 9301 + 49297) % 233280) / 233280;
  return Math.floor(capped * ratio);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const isRetryable = (e) => !NON_RETRYABLE_CODES.includes(e?.code);
const idFor = (r) => createHash('sha256').update(`${r.asset}:${r.price}:${r.source}:${r.observedAt}`).digest('hex');

function validateInput(input) {
  if (!input || typeof input !== 'object') throw new Error('invalid input');
  const { asset, price, source } = input;
  if (typeof asset !== 'string' || asset.length === 0) throw new Error('INVALID_ASSET');
  if (typeof source !== 'string' || source.length === 0) throw new Error('INVALID_SOURCE');
  if (typeof price !== 'number' || !Number.isFinite(price) || price <= 0) {
    const err = new Error('INVALID_PRICE'); err.code = 'INVALID_ASSET'; throw err;
  }
}

function normalizeRequest(input) {
  const observedAt = 'observedAt' in input ? input.observedAt : input.timestamp;
  if (typeof observedAt !== 'number' || !Number.isFinite(observedAt)) throw new Error('INVALID_OBSERVED_AT');
  const idempotencyKey = input.idempotencyKey ?? `${input.asset}:${input.price}:${input.source}:${observedAt}`;
  return { asset: input.asset, price: input.price, source: input.source, observedAt, idempotencyKey };
}

export class ContractUpdater {
  constructor(adapter) {
    this.adapter = adapter ?? {
      submit: async () => { throw new Error('no adapter'); },
      getLatestUpdate: async () => null,
    };
    this.subs = new Map();
    this.latest = new Map();
    this.chains = new Map();
  }

  async submitPriceUpdate(input) {
    validateInput(input);
    const req = normalizeRequest(input);
    const now = Date.now();
    if (req.observedAt > now + FUTURE_SKEW_MS) throw new Error('PRICE_STALE');
    if (req.observedAt < now - STALE_MS) throw new Error('PRICE_STALE');
    return this.enqueue(req.asset, () => this.process(req));
  }

  enqueue(asset, task) {
    const prev = this.chains.get(asset) ?? Promise.resolve();
    const next = prev.then(task, task);
    // Keep the chain alive even if the task rejects, so subsequent updates
    // for the same asset can still run. The caller still sees the rejection.
    this.chains.set(asset, next.catch(() => {}));
    return next;
  }

  async process(req) {
    const id = req.idempotencyKey ?? idFor(req);
    const existing = this.subs.get(id);
    if (existing && !TERMINAL_STATU[S.includes(existing.status)) return existing;

    const last = this.latest.get(req.asset);
    if (last && last.status === 'CONFIRMED' && last.price === req.price) return last;

    const sub = {
      id,
      asset: req.asset,
      price: req.price,
      source: req.source,
      observedAt: req.observedAt,
      createdAt: Date.now(),
      status: 'PENDING',
      attempts: existing?.status === 'FAILED' ? existing.attempts : 0,
      error: existing?.error,
      expiresAt: Date.now() + TIMEOUT_MS,
    };
    this.subs.set(id, sub);

    try {
      await this.execute(sub);
      this.latest.set(req.asset, sub);
      return sub;
    } catch (e) {
      // Only clear the latest entry if it still points at this failed submission.
      // This prevents a concurrent successful update from being clobbered.
      const current = this.latest.get(req.asset);
      if (current === sub) this.latest.delete(req.asset);
      throw e;
    }
  }

  async execute(sub) {
    while (sub.attempts <= MAX_RETRIES && Date.now() < sub.expiresAt) {
      sub.attempts++;
      sub.lastAttemptAt = Date.now();
      try {
        const on = await this.adapter.getLatestUpdate(sub.asset);
        if (on && on.price === sub.price && on.timestamp >= sub.observedAt) {
          sub.status = 'CONFIRMED';
          sub.txHash = on.txHash;
          return sub;
        }
        const res = await this.adapter.submit(sub);
        if (!res || typeof res.txHash !== 'string') {
          const err = new Error('INVALID_ADAPTER_RESPONSE');
          err.code = 'INVALID_ASSET';
          throw err;
        }
        sub.status = 'CONFIRMED';
        sub.txHash = res.txHash;
        return sub;
      } catch (e) {
        sub.error = e instanceof Error ? e.message : String(e);
        const retry = isRetryable(e);
        if (!retry || sub.attempts > MAX_RETRIES || Date.now() >= sub.expiresAt) {
          sub.status = retry ? 'FAILED' : 'REJECTED';
          throw new Error(sub.error);
        }
        await sleep(calculateJitterDelay(sub.attempts - 1));
      }
    }
    sub.status = 'FAILED';
    throw new Error(sub.error ?? 'timeout');
  }
}
