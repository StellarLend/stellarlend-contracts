import { config } from '../config';

export function calculateJitterDelay(
  attempt: number, 
  base: number = config.backoffBaseMs, 
  cap: number = config.backoffCapMs
): number {
  const temp = base * Math.pow(2, attempt);
  const cappedDelay = Math.min(cap, temp);
  return Math.floor(Math.random() * cappedDelay);
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class ContractUpdater {
  async submitPriceUpdate(priceData: any): Promise<void> {
    let attempt = 0;
    const maxRetries = config.maxRetries;

    while (attempt <= maxRetries) {
      try {
        // Core contract invocation logic runs here
        return; 
      } catch (error) {
        if (attempt === maxRetries) {
          throw new Error(`Failed to submit price update after ${maxRetries} retries: ${error}`);
        }

        const delay = calculateJitterDelay(attempt);
        console.warn(`Transient RPC error. Retrying attempt ${attempt + 1}/${maxRetries} after ${delay}ms...`);
        
        await sleep(delay);
        attempt++;
      }
    }
  }
}
