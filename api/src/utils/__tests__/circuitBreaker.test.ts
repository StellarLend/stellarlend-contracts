import CircuitBreaker from '../circuitBreaker';

describe('CircuitBreaker', () => {
  let now = 1000000;
  beforeEach(() => {
    now = 1000000;
    jest.spyOn(Date, 'now').mockImplementation(() => now);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('starts CLOSED and records metrics', () => {
    const cb = new CircuitBreaker({ windowMs: 60000, failureThreshold: 0.5, minRequests: 2 });
    expect(cb.getState()).toBe('CLOSED');
    expect(cb.getMetrics().total).toBe(0);
  });

  it('opens when failure rate exceeds threshold', async () => {
    const cb = new CircuitBreaker({ windowMs: 60000, failureThreshold: 0.5, minRequests: 2, openMs: 1000 });

    // two failures -> rate 1.0
    await expect(cb.exec(async () => { throw new Error('err1'); })).rejects.toThrow();
    now += 100;
    await expect(cb.exec(async () => { throw new Error('err2'); })).rejects.toThrow();

    expect(cb.getMetrics().failures).toBeGreaterThanOrEqual(2);
    expect(cb.getState()).toBe('OPEN');

    // while open, exec should fail fast
    await expect(cb.exec(async () => 'ok')).rejects.toThrow('Circuit is open');
  });

  it('transitions to HALF_OPEN after openMs and recovers on success', async () => {
    const cb = new CircuitBreaker({ windowMs: 60000, failureThreshold: 0.5, minRequests: 2, openMs: 1000, halfOpenMaxTrial: 1 });

    // trip it
    await expect(cb.exec(async () => { throw new Error('err1'); })).rejects.toThrow();
    now += 10;
    await expect(cb.exec(async () => { throw new Error('err2'); })).rejects.toThrow();
    expect(cb.getState()).toBe('OPEN');

    // advance time beyond openMs
    now += 2000;
    expect(cb.getState()).toBe('HALF_OPEN');

    // half-open trial success should reset to CLOSED
    const res = await cb.exec(async () => 'ok');
    expect(res).toBe('ok');
    expect(cb.getState()).toBe('CLOSED');
  });

  it('re-opens on half-open failure', async () => {
    const cb = new CircuitBreaker({ windowMs: 60000, failureThreshold: 0.5, minRequests: 2, openMs: 1000, halfOpenMaxTrial: 1 });

    await expect(cb.exec(async () => { throw new Error('err1'); })).rejects.toThrow();
    now += 10;
    await expect(cb.exec(async () => { throw new Error('err2'); })).rejects.toThrow();
    expect(cb.getState()).toBe('OPEN');

    now += 2000; // move to half-open
    expect(cb.getState()).toBe('HALF_OPEN');

    await expect(cb.exec(async () => { throw new Error('err3'); })).rejects.toThrow();
    expect(cb.getState()).toBe('OPEN');
  });
});
