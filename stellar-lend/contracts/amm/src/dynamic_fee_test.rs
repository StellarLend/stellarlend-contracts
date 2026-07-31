// This file intentionally left blank — dynamic fee tier scaffolding removed.
// Fee configuration is handled exclusively via set_fee_bps / get_fee_bps,
// which read/write KEY_FEE_BPS.  The fee-tier machinery (FeeTier struct,
// FEE_TIERS_KEY, set_fee_tiers/get_fee_tiers) was dead code: the free
// functions were positioned outside #[contractimpl] and thus were never
// exposed as deployable Soroban entry points, and the swap functions
// never consulted them (they use KEY_FEE_BPS directly).
//
// See DYNAMIC_FEE.md for the current fee management documentation.
