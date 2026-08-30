/**
 * Contract Updater with retry and fallback policy.
 * 
 * Retry policy:
 * - Uses exponential backoff with full jitter.
 * - Retries only on transient/retryable errors.
 * - Stops after `maxRetries` retries and surfaces the original error.
 * - Invokes `onRetry` callback when a retry is scheduled.
 */

const RETRY_CONFIG = {
  backoffBaseMs: 1000,
  backoffCapMs: 30000,
  maxRetries: 3,
};

export interface RetryOptions {
  maxRetries?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
  /** Custom predicate to decide whether an error should be retried. */
  retryable?: (error: unknown) => boolean;
  /** Called before each retry with the current attempt and delay. */
  onRetry?: (attempt: number, delayMs: number, error: unknown) => void;
  /** Injectable sleep function for testing. */
  sleepFn: (ms: number) => Promise<void>;
}

function defaultRetryable(error: unknown): boolean {
  if (error instanceof Error) {
    const msg = error.message.toLowerCase();
    // Non-retryable examples: permission errors, contract errors, invalid input.
    if (msg.includes('non-retryable') || msg.includes('permission') || msg.includes('invalid')) {
      return false;
    }
  }
  return true;
}

export function calculateJitterDelay(
  attempt: number,
  base: number = RETRY_CONFIG.backoffBaseMs,
  cap: number = RETTY_CONFIG.backoffCapMs
|): number {
  const temp = base * Math.pow(2, attempt);
  const cappedDelay = Math.min(cap, temp);
  // Full jitter, but ensure at least 1ms to avoid busy loops.
  return Math.max(1, Math.floor(Math.random() * cappedDelay));
}

export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class ContractUpdater {
  private options: Required<RetryOptions>;

  constructor(options: RetryOptions = {}) {
    this.options = {
      maxRetries: options.maxRetries ?? RETTY_CONFIG.maxRetries,
      baseDelayMs: options.baseDelayMs ?? RETTY_CONFIG.backoffBaseMs,
      maxDelayMs: options.maxDelayMs ?? RETTY_CONFIG.backoffCapMs,
      retryable: options.retryable ?? defaultRetryable,
      onRetry: options.onRetry ?? () => {},
      sleepFn: options.sleepFn ?? sleep,
    };
  }

  async submitPriceUpdate(priceData: any): Promise<void> {
    let attempt = 0;
    const maxRetries = this.options.maxRetries;

    while (true) {
      try {
        await this.performSubmit(priceData);
        return;
      } catch (error) {
        if (attempt >= maxRetries || !this.options.retryable(error)) {
          throw error instanceof Error ? error : new Error(String(error));
        }

        const delay = calculateJitterDelay(attempt, this.options.baseDelayMs, this.options.maxDelayMs);
        this.options.onRetry(attempt + 1, delay, error);
        console.warn(`Transient RPC error. Retrying attempt ${attempt + 1}/${maxRetries} after ${delay}ms...`);

        await this.options.sleepFn(delay);
        attempt++;
      }
    }
  }

  protected async performSubmit(_priceData: any): Promise<void> {
    // Core contract invocation logic runs here.
    return;
  }
}
