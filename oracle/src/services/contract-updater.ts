const RETRY_CONFIG = {
  backoffBaseMs: 1000,
  backoffCapMs: 30000,
  maxRetries: 3,
};

export function calculateJitterDelay(
  attempt: number, 
  base: number = RETRY_CONFIG.backoffBaseMs, 
  cap: number = RETRY_CONFIG.backoffCapMs
): number {
  const temp = base * Math.pow(2, attempt);
  const cappedDelay = Math.min(cap, temp);
  return Math.floor(Math.random() * cappedDelay);
}

export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class ContractUpdater {
  async submitPriceUpdate(priceData: any): Promise<void> {
    let attempt = 0;
    const maxRetries = RETRY_CONFIG.maxRetries;

    while (true) {
      try {
        await this.performSubmit(priceData);
        return;
      } catch (error) {
        if (attempt >= maxRetries) {
          throw new Error(`Failed to submit price update after ${maxRetries} retries: ${error}`);
        }

        const delay = calculateJitterDelay(attempt);
        console.warn(`Transient RPC error. Retrying attempt ${attempt + 1}/${maxRetries} after ${delay}ms...`);
        
        await sleep(delay);
        attempt++;
      }
    }
  }

  protected async performSubmit(_priceData: any): Promise<void> {
    // Core contract invocation logic runs here.
    return;
  }
}
