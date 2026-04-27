/**
 * Integration Test
 * 
 * Run this locally to verify the oracle service works with the APIs.
 * Usage: npx tsx tests/live-test.ts
 */

import dotenv from 'dotenv';
dotenv.config();

import { createCoinGeckoProvider } from '../src/providers/coingecko.js';
import { createBinanceProvider } from '../src/providers/binance.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { createAggregator } from '../src/services/price-aggregator.js';

async function testLive() {
    console.log('\n🚀 StellarLend Oracle - Live Integration Test\n');
    console.log('='.repeat(55));

    // Create providers
    const coingecko = createCoinGeckoProvider(process.env.COINGECKO_API_KEY);
    const binance = createBinanceProvider();

    console.log('\n📊 Testing Individual Providers...\n');

    // Test CoinGecko
    console.log('CoinGecko:');
    try {
        const xlm = await coingecko.fetchPrice('XLM');
        console.log(`  ✅ XLM = $${xlm.price.toFixed(4)}`);

        const btc = await coingecko.fetchPrice('BTC');
        console.log(`  ✅ BTC = $${btc.price.toLocaleString()}`);
    } catch (err) {
        console.log(`  ❌ Error: ${err instanceof Error ? err.message : err}`);
    }

    // Test Binance
    console.log('\nBinance:');
    try {
        const xlm = await binance.fetchPrice('XLM');
        console.log(`  ✅ XLM = $${xlm.price.toFixed(4)}`);

        const btc = await binance.fetchPrice('BTC');
        console.log(`  ✅ BTC = $${btc.price.toLocaleString()}`);
    } catch (err) {
        console.log(`  ❌ Error: ${err instanceof Error ? err.message : err}`);
    }

    // Test Aggregator with all providers
    console.log('\n📊 Testing Price Aggregator (All Providers)...\n');

    const validator = createValidator();
    const cache = createPriceCache(60);
    const aggregator = createAggregator(
        [coingecko, binance],
        validator,
        cache,
        { minSources: 1 }
    );

    try {
        const prices = await aggregator.getPrices(['XLM', 'BTC', 'ETH']);

        console.log('Aggregated Prices:');
        for (const [asset, data] of prices) {
            const priceNum = Number(data.price) / 1_000_000;
            console.log(`  ${asset}: $${priceNum.toFixed(asset === 'XLM' ? 4 : 2)} (confidence: ${data.confidence.toFixed(0)}%, sources: ${data.sources.length})`);
        }
    } catch (err) {
        console.log(`  ❌ Aggregation Error: ${err instanceof Error ? err.message : err}`);
    }

    console.log('\n' + '='.repeat(55));
    console.log('✨ Test complete!\n');
}

testLive().catch(console.error);
