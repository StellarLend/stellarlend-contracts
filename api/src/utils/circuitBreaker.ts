export type CircuitState = 'CLOSED' | 'OPEN' | 'HALF_OPEN';

interface CBOptions {
  windowMs?: number;
  failureThreshold?: number; // fraction, e.g., 0.5
  minRequests?: number;
  openMs?: number;
  halfOpenMaxTrial?: number;
}

export class CircuitBreaker {
  private windowMs: number;
  private failureThreshold: number;
  private minRequests: number;
  private openMs: number;
  private halfOpenMaxTrial: number;

  private state: CircuitState = 'CLOSED';
  private openedAt = 0;
  private events: Array<{ ts: number; success: boolean }> = [];
  private halfOpenTrials = 0;

  constructor(opts: CBOptions = {}) {
    this.windowMs = opts.windowMs ?? 60000;
    this.failureThreshold = opts.failureThreshold ?? 0.5;
    this.minRequests = opts.minRequests ?? 5;
    this.openMs = opts.openMs ?? 30000;
    this.halfOpenMaxTrial = opts.halfOpenMaxTrial ?? 2;
  }

  private now() {
    return Date.now();
  }

  private purgeOld() {
    const cutoff = this.now() - this.windowMs;
    while (this.events.length && this.events[0].ts < cutoff) {
      this.events.shift();
    }
  }

  private evaluateState() {
    if (this.state === 'OPEN') {
      if (this.now() - this.openedAt >= this.openMs) {
        this.state = 'HALF_OPEN';
        this.halfOpenTrials = 0;
      }
      return;
    }

    this.purgeOld();
    const total = this.events.length;
    if (total < this.minRequests) return;

    const failures = this.events.filter(e => !e.success).length;
    const rate = failures / total;

    if (rate >= this.failureThreshold) {
      this.state = 'OPEN';
      this.openedAt = this.now();
    }
  }

  public record(success: boolean) {
    this.events.push({ ts: this.now(), success });
    this.evaluateState();
    if (this.state === 'HALF_OPEN') {
      if (success) {
        this.halfOpenTrials += 1;
        if (this.halfOpenTrials >= this.halfOpenMaxTrial) {
          this.reset();
        }
      } else {
        this.trip();
      }
    }
  }

  private trip() {
    this.state = 'OPEN';
    this.openedAt = this.now();
  }

  private reset() {
    this.state = 'CLOSED';
    this.events = [];
    this.halfOpenTrials = 0;
  }

  public getState(): CircuitState {
    this.evaluateState();
    return this.state;
  }

  public getMetrics() {
    this.purgeOld();
    const total = this.events.length;
    const failures = this.events.filter(e => !e.success).length;
    const rate = total > 0 ? failures / total : 0;
    return {
      state: this.getState(),
      windowMs: this.windowMs,
      total,
      failures,
      failureRate: rate,
    };
  }

  public async exec<T>(fn: () => Promise<T>): Promise<T> {
    const state = this.getState();
    if (state === 'OPEN') {
      throw new Error('Circuit is open');
    }

    try {
      const res = await fn();
      this.record(true);
      return res;
    } catch (err) {
      this.record(false);
      throw err;
    }
  }
}

export default CircuitBreaker;
