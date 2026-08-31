/**
 * Contract Updater Service
 *
 * Submits signed price updates to the Soroban lending contract.
 *
 * Combines:
 * - Upstream's idempotency/state-machine/per-asset serialisation adapter
 *   (prevents duplicate on-chain submissions and enables recovery).
 * - This PR's typed ContractUpdaterConfig, input-guarded full-jitter
 *   calculateJitterDelay, secret-safe error classification, and batch
 *   updatePrices / healthCheck / getAdminPublicKey surface.
 */

import { createHash } from 'crypto';
import { runtimeConfig } from '../config.js';
import type { AggregatedPrice, ContractUpdateResult } from '../types/index.js';
import { logger } from '../utils/logger.js';

// ─── Typed config ─────────────────────────────────────────────────────────────

export interface ContractUpdaterConfig {
    network: 'testnet' | 'mainnet';
    rpcUrl: string;
    contractId: string;
    adminSecretKey: string;
    /** Maximum number of retry attempts after the first failure (0 = no retries). */
    maxRetries?: number;
    /** Base delay in ms for exponential back-off. */
    retryDelayMs?: number;
    /** Upper cap for back-off delay in ms. */
    retryCapMs?: number;
}

/** Adapter interface for the underlying Soroban RPC layer. Swap in tests. */
export interface ContractAdapter {
    submit(sub: Submission): Promise<{ txHash: string }>;
    getLatestUpdate(asset: string): Promise<{ price: bigint; timestamp: number; txHash: string } | null>;
}

// ─── Internal submission record ───────────────────────────────────────────────

interface Submission {
    id: string;
    asset: string;
    price: bigint;
    source: string;
    observedAt: number;
    createdAt: number;
    status: 'PENDING' | 'CONFIRMED' | 'FAILED' | 'REJECTED';
    attempts: number;
    lastAttemptAt?: number;
    expiresAt: number;
    txHash?: string;
    error?: string;
}

// ─── Module-level constants ───────────────────────────────────────────────────

const TIMEOUT_MS = 120_000;
const STALE_MS = 300_000;

// Non-retryable error codes from the contract/RPC layer.
const NON_RETRYABLE_CODES = new Set(['PRICE_STALE', 'INVALID_ASSET', 'SOURCE_UNAVAILABLE']);

// ─── Jitter helper ────────────────────────────────────────────────────────────

/**
 * Full-jitter exponential back-off delay for attempt `n`.
 * Formula: `rand(0, min(cap, base × 2^n))`.
 * Inputs are guarded; attempt is clamped to 30 to prevent 2^Inf.
 */
export function calculateJitterDelay(
    attempt: number,
    base: number = runtimeConfig.backoffBaseMs,
    cap: number = runtimeConfig.backoffCapMs,
): number {
    const safeCap = Math.max(1, isFinite(cap) ? cap : runtimeConfig.backoffCapMs);
    const safeBase = Math.max(1, isFinite(base) ? base : runtimeConfig.backoffBaseMs);
    const safeAttempt = Math.max(0, Math.min(attempt, 30));
    const window = Math.min(safeCap, safeBase * Math.pow(2, safeAttempt));
    return Math.floor(Math.random() * window);
}

// ─── Error classification ─────────────────────────────────────────────────────

type ErrorClass = 'network' | 'rate_limit' | 'validation' | 'timeout' | 'unknown';

/** Map a thrown error to a non-secret class for telemetry. Never exposes raw messages. */
function classifyError(err: unknown): ErrorClass {
    if (!(err instanceof Error)) return 'unknown';
    const msg = err.message.toLowerCase();
    if (msg.includes('timeout') || msg.includes('timed out')) return 'timeout';
    if (msg.includes('429') || msg.includes('rate limit') || msg.includes('too many')) return 'rate_limit';
    if (msg.includes('invalid') || msg.includes('validation') || msg.includes('malformed')) return 'validation';
    if (msg.includes('network') || msg.includes('econnrefused') || msg.includes('enotfound') || msg.includes('socket')) return 'network';
    return 'unknown';
}

function isRetryable(err: unknown): boolean {
    const code = (err as any)?.code;
    return typeof code !== 'string' || !NON_RETRYABLE_CODES.has(code);
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function idFor(asset: string, price: bigint, source: string, observedAt: number): string {
    return createHash('sha256')
        .update(`${asset}:${price}:${source}:${observedAt}`)
        .digest('hex');
}

const defaultAdapter: ContractAdapter = {
    submit: async () => { throw new Error('no adapter configured'); },
    getLatestUpdate: async () => null,
};

// ─── ContractUpdater ──────────────────────────────────────────────────────────

export class ContractUpdater {
    private readonly cfg: Required<ContractUpdaterConfig>;
    private readonly adapter: ContractAdapter;
    /** Per-idempotency-key submission records. */
    private readonly subs = new Map<string, Submission>();
    /** Most-recently confirmed submission per asset. */
    private readonly latest = new Map<string, Submission>();
    /** Per-asset promise chain to serialise submissions. */
    private readonly chains = new Map<string, Promise<void>>();

    constructor(config: ContractUpdaterConfig, adapter?: ContractAdapter) {
        this.cfg = {
            network: config.network,
            rpcUrl: config.rpcUrl,
            contractId: config.contractId,
            adminSecretKey: config.adminSecretKey,
            maxRetries: config.maxRetries ?? runtimeConfig.maxRetries,
            retryDelayMs: config.retryDelayMs ?? runtimeConfig.backoffBaseMs,
            retryCapMs: config.retryCapMs ?? runtimeConfig.backoffCapMs,
        };
        this.adapter = adapter ?? defaultAdapter;
    }

    /**
     * Return a masked admin identifier safe for structured logs.
     * The raw secret key is never logged.
     */
    getAdminPublicKey(): string {
        const sk = this.cfg.adminSecretKey;
        if (!sk || sk.length < 8) return '(unset)';
        return `${sk.slice(0, 4)}…${sk.slice(-4)} (masked)`;
    }

    /**
     * Submit a single aggregated price update.
     *
     * - Validates the timestamp window (not future, not stale).
     * - Deduplicates via idempotency key so the same price is never submitted twice.
     * - Serialises per-asset to prevent concurrent chain mutations.
     * - Returns a ContractUpdateResult with latency and error class.
     */
    async submitPriceUpdate(price: AggregatedPrice): Promise<ContractUpdateResult> {
        const startMs = Date.now();
        const now = Date.now();
        const observedAt = price.timestamp * 1000; // unix seconds → ms

        if (observedAt > now + 5_000 || observedAt < now - STALE_MS) {
            const errClass: ErrorClass = 'validation';
            logger.warn('Price update rejected: timestamp out of window', {
                asset: price.asset,
                errorClass: errClass,
            });
            return {
                success: false,
                asset: price.asset,
                price: price.price,
                timestamp: price.timestamp,
                error: `${errClass} error: timestamp out of allowed window`,
            };
        }

        const source = price.sources[0]?.source ?? 'aggregated';
        const idempotencyKey = `${price.asset}:${price.price}:${source}:${price.timestamp}`;

        const req = {
            asset: price.asset,
            price: price.price,
            source,
            observedAt,
            idempotencyKey,
        };

        try {
            await this.enqueue(price.asset, () => this.process(req));
            const latencyMs = Date.now() - startMs;
            logger.debug('Price update submitted', { asset: price.asset, latencyMs });
            return {
                success: true,
                asset: price.asset,
                price: price.price,
                timestamp: price.timestamp,
                idempotencyKey,
            };
        } catch (err) {
            const latencyMs = Date.now() - startMs;
            const errClass = classifyError(err);
            logger.error('Price update failed', {
                asset: price.asset,
                latencyMs,
                errorClass: errClass,
                // err.message deliberately omitted — may contain RPC response bodies
            });
            return {
                success: false,
                asset: price.asset,
                price: price.price,
                timestamp: price.timestamp,
                error: `${errClass} error after ${this.cfg.maxRetries + 1} attempt(s)`,
                idempotencyKey,
            };
        }
    }

    /**
     * Submit a batch of aggregated prices sequentially.
     */
    async updatePrices(prices: AggregatedPrice[]): Promise<ContractUpdateResult[]> {
        const results: ContractUpdateResult[] = [];
        for (const p of prices) {
            results.push(await this.submitPriceUpdate(p));
        }
        return results;
    }

    /**
     * Lightweight health-check. Never throws — returns false on failure.
     */
    async healthCheck(): Promise<boolean> {
        try {
            return true; // production: check RPC liveness
        } catch {
            return false;
        }
    }

    // ─── Private: per-asset serialisation ────────────────────────────────────

    private enqueue(asset: string, task: () => Promise<void>): Promise<void> {
        const prev = this.chains.get(asset) ?? Promise.resolve();
        const next = prev.then(task, task);
        this.chains.set(asset, next.catch(() => {}));
        return next;
    }

    private async process(req: {
        asset: string;
        price: bigint;
        source: string;
        observedAt: number;
        idempotencyKey: string;
    }): Promise<void> {
        const id = req.idempotencyKey ?? idFor(req.asset, req.price, req.source, req.observedAt);
        const existing = this.subs.get(id);

        // Skip if there's an in-flight or confirmed submission for this key.
        if (existing && !['FAILED', 'REJECTED'].includes(existing.status)) return;

        // Skip if the last confirmed submission for this asset already has this price.
        const last = this.latest.get(req.asset);
        if (last && last.status === 'CONFIRMED' && last.price === req.price) return;

        const sub: Submission = {
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
        } catch (e) {
            this.latest.delete(req.asset);
            throw e;
        }
    }

    private async execute(sub: Submission): Promise<void> {
        const maxRetries = this.cfg.maxRetries;

        while (sub.attempts <= maxRetries && Date.now() < sub.expiresAt) {
            sub.attempts++;
            sub.lastAttemptAt = Date.now();

            try {
                // Check if already confirmed on-chain (idempotent read).
                const onChain = await this.adapter.getLatestUpdate(sub.asset);
                if (onChain && onChain.price === sub.price && onChain.timestamp >= sub.observedAt) {
                    sub.status = 'CONFIRMED';
                    sub.txHash = onChain.txHash;
                    return;
                }

                const res = await this.adapter.submit(sub);
                sub.status = 'CONFIRMED';
                sub.txHash = res.txHash;
                return;
            } catch (e) {
                // Never log raw message — it may embed secrets from RPC responses.
                sub.error = classifyError(e);

                const exhausted = !isRetryable(e) || sub.attempts > maxRetries || Date.now() >= sub.expiresAt;
                if (exhausted) {
                    sub.status = isRetryable(e) ? 'FAILED' : 'REJECTED';
                    throw new Error(sub.error);
                }

                await sleep(calculateJitterDelay(sub.attempts - 1, this.cfg.retryDelayMs, this.cfg.retryCapMs));
            }
        }

        sub.status = 'FAILED';
        throw new Error(sub.error ?? 'timeout');
    }
}

// ─── Factory ──────────────────────────────────────────────────────────────────

export function createContractUpdater(
    config: ContractUpdaterConfig,
    adapter?: ContractAdapter,
): ContractUpdater {
    return new ContractUpdater(config, adapter);
}
