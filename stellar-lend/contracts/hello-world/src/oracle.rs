#![allow(dead_code)]
use soroban_sdk::{contracttype, vec, Address, Env, IntoVal, Symbol, Vec};

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct OracleSource {
    pub addr: Address,
    pub weight: i128, // relative weight (sum can be arbitrary)
    pub last_heartbeat: u64,
}

impl OracleSource {
    pub fn new(addr: Address, weight: i128, last_heartbeat: u64) -> Self {
        Self {
            addr,
            weight,
            last_heartbeat,
        }
    }
}

pub struct OracleStorage;

impl OracleStorage {
    fn sources_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_sources")
    }
    fn heartbeat_ttl_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_heartbeat_ttl")
    }
    fn mode_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_mode")
    }
    fn perf_count_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_perf_count")
    }
    fn deviation_bps_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_deviation_bps")
    }
    fn trim_count_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_trim_count")
    }
    fn twap_window_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_twap_window")
    }
    fn price_cache_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_price_cache")
    }
    fn price_cache_ttl_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle_price_cache_ttl")
    }

    pub fn get_sources(env: &Env, asset: &Address) -> Vec<OracleSource> {
        let key = (Self::sources_key(env), asset.clone());
        env.storage()
            .instance()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env))
    }

    pub fn put_sources(env: &Env, asset: &Address, sources: &Vec<OracleSource>) {
        let key = (Self::sources_key(env), asset.clone());
        env.storage().instance().set(&key, sources);
    }

    pub fn get_heartbeat_ttl(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&Self::heartbeat_ttl_key(env))
            .unwrap_or(300)
    }

    pub fn set_heartbeat_ttl(
        env: &Env,
        caller: &Address,
        ttl: u64,
    ) -> Result<(), crate::ProtocolError> {
        crate::UserManager::require_admin(env, caller)?;
        env.storage()
            .instance()
            .set(&Self::heartbeat_ttl_key(env), &ttl);
        Ok(())
    }

    pub fn set_mode(env: &Env, caller: &Address, mode: i128) -> Result<(), crate::ProtocolError> {
        crate::UserManager::require_admin(env, caller)?;
        env.storage().instance().set(&Self::mode_key(env), &mode);
        Ok(())
    }
    pub fn get_mode(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&Self::mode_key(env))
            .unwrap_or(0)
    } // 0=median,1=twap

    pub fn inc_perf(env: &Env) -> i128 {
        let cur: i128 = env
            .storage()
            .instance()
            .get(&Self::perf_count_key(env))
            .unwrap_or(0)
            + 1;
        env.storage()
            .instance()
            .set(&Self::perf_count_key(env), &cur);
        cur
    }

    /// Maximum deviation from the median allowed before outlier rejection, in bps (1/10000)
    pub fn get_deviation_bps(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&Self::deviation_bps_key(env))
            .unwrap_or(500) // default 5%
    }
    pub fn set_deviation_bps(env: &Env, bps: i128) {
        env.storage()
            .instance()
            .set(&Self::deviation_bps_key(env), &bps);
    }

    /// Number of highest/lowest samples to trim before aggregation when using median mode
    pub fn get_trim_count(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&Self::trim_count_key(env))
            .unwrap_or(1)
    }
    pub fn set_trim_count(env: &Env, count: i128) {
        env.storage()
            .instance()
            .set(&Self::trim_count_key(env), &count);
    }

    /// TWAP window size in samples (conceptual window; sources provide one price each fetch)
    pub fn get_twap_window(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&Self::twap_window_key(env))
            .unwrap_or(5)
    }
    pub fn set_twap_window(env: &Env, window: i128) {
        env.storage()
            .instance()
            .set(&Self::twap_window_key(env), &window);
    }

    // Aggregated price cache helpers
    pub fn get_price_cache(env: &Env) -> soroban_sdk::Map<Address, (i128, u64)> {
        env.storage()
            .instance()
            .get(&Self::price_cache_key(env))
            .unwrap_or_else(|| soroban_sdk::Map::new(env))
    }
    pub fn put_price_cache(env: &Env, map: &soroban_sdk::Map<Address, (i128, u64)>) {
        env.storage()
            .instance()
            .set(&Self::price_cache_key(env), map);
    }
    pub fn get_price_cache_ttl(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&Self::price_cache_ttl_key(env))
            .unwrap_or(30)
    }
    pub fn set_price_cache_ttl(env: &Env, ttl: u64) {
        env.storage()
            .instance()
            .set(&Self::price_cache_ttl_key(env), &ttl);
    }
}

pub struct Oracle;

impl Oracle {
    /// Register or update an oracle source for an asset
    pub fn set_source(
        env: &Env,
        caller: &Address,
        asset: &Address,
        source: OracleSource,
    ) -> Result<(), crate::ProtocolError> {
        crate::UserManager::require_admin(env, caller)?;
        let list = OracleStorage::get_sources(env, asset);
        // Replace if exists
        let mut replaced = false;
        let mut out: Vec<OracleSource> = Vec::new(env);
        for s in list.iter() {
            if s.addr == source.addr {
                out.push_back(source.clone());
                replaced = true;
            } else {
                out.push_back(s);
            }
        }
        if !replaced {
            out.push_back(source);
        }
        OracleStorage::put_sources(env, asset, &out);
        Ok(())
    }

    /// Remove a source
    pub fn remove_source(
        env: &Env,
        caller: &Address,
        asset: &Address,
        addr: &Address,
    ) -> Result<(), crate::ProtocolError> {
        crate::UserManager::require_admin(env, caller)?;
        let list = OracleStorage::get_sources(env, asset);
        let mut out: Vec<OracleSource> = Vec::new(env);
        for s in list.iter() {
            if s.addr != *addr {
                out.push_back(s);
            }
        }
        OracleStorage::put_sources(env, asset, &out);
        Ok(())
    }

    /// Fetch prices from all sources (stubbed as calling `get_price()` on source contracts)
    /// Policies:
    /// - Staleness: drop sources whose last_heartbeat is older than TTL
    /// - Non-positive prices are ignored
    pub fn fetch_prices(env: &Env, asset: &Address) -> Vec<i128> {
        let list = OracleStorage::get_sources(env, asset);
        let ttl = OracleStorage::get_heartbeat_ttl(env);
        let now = env.ledger().timestamp();
        let mut prices: Vec<i128> = Vec::new(env);
        for s in list.iter() {
            if now.saturating_sub(s.last_heartbeat) > ttl {
                continue;
            }
            // Try calling a standard oracle interface: fn get_price(asset: Address) -> i128
            let args = vec![env, asset.clone().into_val(env)];
            let price: i128 = env.invoke_contract(&s.addr, &Symbol::new(env, "get_price"), args);
            if price > 0 {
                prices.push_back(price);
            }
        }
        prices
    }

    /// Aggregate prices using configured policy.
    /// - mode 0: median with configurable trim and deviation filter
    /// - mode 1: TWAP approximation over current fetch with configurable window size (average)
    pub fn aggregate_price(env: &Env, asset: &Address) -> Option<i128> {
        // Cache check
        let ttl = OracleStorage::get_price_cache_ttl(env);
        let now = env.ledger().timestamp();
        let mut cache = OracleStorage::get_price_cache(env);
        if let Some((cached, ts)) = cache.get(asset.clone()) {
            if now.saturating_sub(ts) <= ttl {
                // cache hit
                crate::ProtocolEvent::CacheUpdated(
                    Symbol::new(env, "oracle_price_cache"),
                    Symbol::new(env, "hit"),
                )
                .emit(env);
                return Some(cached);
            } else {
                // evict stale
                cache.remove(asset.clone());
                OracleStorage::put_price_cache(env, &cache);
                crate::ProtocolEvent::CacheUpdated(
                    Symbol::new(env, "oracle_price_cache"),
                    Symbol::new(env, "evict"),
                )
                .emit(env);
            }
        }

        let mut prices = Self::fetch_prices(env, asset);
        OracleStorage::inc_perf(env);
        let n_usize = prices.len() as usize;
        if n_usize == 0 {
            return None;
        }
        let mode = OracleStorage::get_mode(env);
        if mode == 1 {
            // TWAP approximation: simple average for now; window size informs minimal sample need
            let window = OracleStorage::get_twap_window(env).max(1) as usize;
            let use_n = core::cmp::min(n_usize, window);
            let mut sum: i128 = 0;
            for i in 0..use_n {
                sum = sum.saturating_add(prices.get(i as u32).unwrap_or(0));
            }
            let out = sum / (use_n as i128);
            cache.set(asset.clone(), (out, now));
            OracleStorage::put_price_cache(env, &cache);
            crate::ProtocolEvent::CacheUpdated(
                Symbol::new(env, "oracle_price_cache"),
                Symbol::new(env, "set"),
            )
            .emit(env);
            return Some(out);
        }

        // Sort ascending (simple O(n^2) acceptable for small n)
        for i in 0..n_usize {
            for j in i + 1..n_usize {
                if prices.get(i as u32).unwrap() > prices.get(j as u32).unwrap() {
                    let a = prices.get(i as u32).unwrap();
                    let b = prices.get(j as u32).unwrap();
                    prices.set(i as u32, b);
                    prices.set(j as u32, a);
                }
            }
        }

        // Trim highest and lowest samples per configuration if enough samples
        let trim = OracleStorage::get_trim_count(env).max(0) as usize;
        let start = if n_usize > trim { trim } else { 0 };
        let end = if n_usize > trim {
            n_usize.saturating_sub(trim)
        } else {
            n_usize
        };
        if end <= start {
            let out = prices.get((n_usize / 2) as u32).unwrap();
            cache.set(asset.clone(), (out, now));
            OracleStorage::put_price_cache(env, &cache);
            crate::ProtocolEvent::CacheUpdated(
                Symbol::new(env, "oracle_price_cache"),
                Symbol::new(env, "set"),
            )
            .emit(env);
            return Some(out);
        }

        // Deviation filter: compute median of trimmed set, then remove values beyond deviation_bps
        let span = end - start;
        let mid = start + span / 2;
        let med = if span % 2 == 1 {
            prices.get(mid as u32).unwrap()
        } else {
            let a = prices.get((mid - 1) as u32).unwrap();
            let b = prices.get(mid as u32).unwrap();
            (a + b) / 2
        };

        let deviation_bps = OracleStorage::get_deviation_bps(env).max(0);
        let mut filtered: Vec<i128> = Vec::new(env);
        for k in start..end {
            let p = prices.get(k as u32).unwrap();
            let diff = (p - med).abs();
            // allow within med * deviation_bps / 10000
            let max_diff = (med.abs().saturating_mul(deviation_bps)).saturating_div(10000);
            if diff <= max_diff {
                filtered.push_back(p);
            }
        }
        let out_final = if filtered.is_empty() {
            med
        } else {
            let m_usize = filtered.len() as usize;
            for i in 0..m_usize {
                for j in i + 1..m_usize {
                    if filtered.get(i as u32).unwrap() > filtered.get(j as u32).unwrap() {
                        let a = filtered.get(i as u32).unwrap();
                        let b = filtered.get(j as u32).unwrap();
                        filtered.set(i as u32, b);
                        filtered.set(j as u32, a);
                    }
                }
            }
            let mid_f = m_usize / 2;
            if m_usize % 2 == 1 {
                filtered.get(mid_f as u32).unwrap()
            } else {
                (filtered.get((mid_f - 1) as u32).unwrap() + filtered.get(mid_f as u32).unwrap())
                    / 2
            }
        };
        cache.set(asset.clone(), (out_final, now));
        OracleStorage::put_price_cache(env, &cache);
        crate::ProtocolEvent::CacheUpdated(
            Symbol::new(env, "oracle_price_cache"),
            Symbol::new(env, "set"),
        )
        .emit(env);
        Some(out_final)
    }
}
