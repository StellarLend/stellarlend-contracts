Title: Wire outbound_nonce_test into bridge lib.rs

Added missing #[cfg(test)] mod outbound_nonce_test; to ensure the test module is compiled and run.

This PR wires the test module and adds a basic test file outbound_nonce_test.rs that verifies peek_outbound_nonce defaults to 0 for a fresh destination.

- wired the test module into stellar-lend/contracts/bridge/src/lib.rs
- added stellar-lend/contracts/bridge/src/outbound_nonce_test.rs

Signed-off-by: Prevail Ugah <Prevailbugah@gmail.com>
