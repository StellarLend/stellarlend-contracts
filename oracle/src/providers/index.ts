/**
 * Exports all price provider implementations and factory functions.
 *
 * Invariants:
 * - The module is a pure re-export surface; it must not introduce
 *   side-effects, mutable state, or conditional exports that could make
 *   import resolution non-deterministic across environments.
 * - Every exported factory must return a fresh instance or throw a
 *   deterministic error for invalid configuration; failures must not be
 *   swallowed or converted into silent undefined values.
 * - Public identifiers and their names are part of the contract and
 *   must remain stable for existing callers.
 */

export { BasePriceProvider } from './base-provider.js';
export { CoinGeckoProvider, createCoinGeckoProvider } from './coingecko.js';
export { BinanceProvider, createBinanceProvider } from './binance.js';
