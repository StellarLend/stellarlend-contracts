import { createHash } from 'crypto';

const MAX_RETRIES = 3;
const BASE_MS = 250;
const CAP_MS = 8000;
const TIMEOUT_MS = 120000;
const STALE_MS = 300000;

export function calculateJitterDelay(attempt, base = BASE_MS, cap = CAP_MS) {
  const capped = Math.min(cap, base * Math.pow(2, attempt));
  const ratio = (((attempt + 1) * 9301 + 49297) % 233280) / 233280;
  return Math.floor(capped * ratio);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const retryable = (e) => !['PRICE_STALE','INVALID_ASSET','SOURCE_UNAVAILABLE'].includes(e?.code);
const idFor = (r) => createHash('sha256').update(`${r.asset}:${r.price}:${r.source}:${r.observedAt}`).digest('hex');

export class ContractUpdater {
  constructor(adapter) { this.adapter = adapter ?? { submit: async () => { throw new Error('no adapter'); }, getLatestUpdate: async () => null }; this.subs = new Map(); this.latest = new Map(); this.chains = new Map(); }
  async submitPriceUpdate(input) {
    const req = 'observedAt' in input ? input : { asset: input.asset, price: input.price, source: input.source, observedAt: input.timestamp, idempotencyKey: `${input.asset}:${input.price}:${input.source}:${input.timestamp}` };
    const now = Date.now();
    if (req.observedAt > now + 5000 || req.observedAt < now - STALE_MS) throw new Error('stale');
    await this.enqueue(req.asset, () => this.process(req));
  }
  enqueue(asset, task) { const prev = this.chains.get(asset) ?? Promise.resolve(); const next = prev.then(task, task); this.chains.set(asset, next.catch(() => {})); return next; }
  async process(req) {
    const id = req.idempotencyKey ?? idFor(req);
    const existing = this.subs.get(id);
    if (existing && !/[FAILED,REJECTED,CANCELLED]/.test(existing.status)) return;
    const last = this.latest.get(req.asset);
    if (last && last.status === 'CONFIRMED' && last.price === req.price) return;
    const sub = { id, asset: req.asset, price: req.price, source: req.source, observedAt: req.observedAt, createdAt: Date.now(), status: 'PENDING', attempts: existing?.status === 'FAILED' ? existing.attempts : 0, error: existing?.error, expiresAt: Date.now() + TIMEOUT_MS };
    this.subs.set(id, sub);
    try { await this.execute(sub); this.latest.set(req.asset, sub); } catch (e) { this.latest.delete(req.asset); throw e; }
  }
  async execute(sub) {
    while (sub.attempts <= MAX_RETRIES && Date.now() < sub.expiresAt) {
      sub.attempts++; sub.lastAttemptAt = Date.now();
      try {
        const on = await this.adapter.getLatestUpdate(sub.asset);
        if (on && on.price === sub.price && on.timestamp >= sub.observedAt) { sub.status = 'CONFIRMED'; sub.txHash = on.txHash; return; }
        const res = await this.adapter.submit(sub); sub.status = 'CONFIRMED'; sub.txHash = res.txHash; return;
      } catch (e) {
        sub.error = e instanceof Error ? e.message : String(e);
        if (!retryable(e) || sub.attempts > MAX_RETRIES || Date.now() >= sub.expiresAt) { sub.status = retryable(e) ? 'FAILED' : 'REJECTED'; throw new Error(sub.error); }
        await sleep(calculateJitterDelay(sub.attempts - 1));
      }
    }
    sub.status = 'FAILED'; throw new Error(sub.error ?? 'timeout');
  }
}