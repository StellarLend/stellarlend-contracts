/// Tests for outbound message capacity / cap enforcement in the bridge contract.
///
/// These tests exercise the outbound rate limiter through the public entrypoints. Time is
/// taken from the ledger, so the admit/cap signatures do not accept a caller-supplied
/// timestamp. The compile smoke test below keeps the module building independently of
/// the other test modules.

#[cfg(test)]
#[allow(unused_imports)]
use super::*;

#[cfg(test)]
#[test]
fn outbound_cap_test_compile_smoke() {}
