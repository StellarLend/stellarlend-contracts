//! Network configuration for Stellar Horizon and Soroban RPC endpoints.
//!
//! This module provides configuration for connecting to different Stellar networks
//! (testnet, mainnet, futurenet) and their corresponding Horizon and Soroban RPC endpoints.

use crate::error::{BlockchainError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Network type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Network {
    /// Stellar testnet
    Testnet,
    /// Stellar mainnet (public network)
    Mainnet,
    /// Stellar futurenet (for testing upcoming features)
    Futurenet,
    /// Custom network with user-defined endpoints
    Custom,
}

impl Network {
    /// Get the network passphrase
    pub fn passphrase(&self) -> &'static str {
        match self {
            Network::Testnet => "Test SDF Network ; September 2015",
            Network::Mainnet => "Public Global Stellar Network ; September 2015",
            Network::Futurenet => "Test SDF Future Network ; October 2022",
            Network::Custom => "Custom Network",
        }
    }

    /// Get the default Horizon URL for this network
    pub fn default_horizon_url(&self) -> &'static str {
        match self {
            Network::Testnet => "https://horizon-testnet.stellar.org",
            Network::Mainnet => "https://horizon.stellar.org",
            Network::Futurenet => "https://horizon-futurenet.stellar.org",
            Network::Custom => "",
        }
    }

    /// Get the default Soroban RPC URL for this network
    pub fn default_soroban_rpc_url(&self) -> &'static str {
        match self {
            Network::Testnet => "https://soroban-testnet.stellar.org",
            Network::Mainnet => "https://soroban-mainnet.stellar.org",
            Network::Futurenet => "https://rpc-futurenet.stellar.org",
            Network::Custom => "",
        }
    }
}

/// Configuration for blockchain clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// Network to connect to
    pub network: Network,

    /// Horizon API endpoint URL
    pub horizon_url: String,

    /// Soroban RPC endpoint URL
    pub soroban_rpc_url: String,

    /// Network passphrase
    pub network_passphrase: String,

    /// HTTP request timeout
    pub request_timeout: Duration,

    /// Maximum number of retries for failed requests
    pub max_retries: usize,

    /// Initial retry delay (in milliseconds)
    pub retry_initial_delay_ms: u64,

    /// Maximum retry delay (in milliseconds)
    pub retry_max_delay_ms: u64,

    /// Retry backoff multiplier
    pub retry_multiplier: f64,

    /// Transaction polling interval (in milliseconds)
    pub tx_poll_interval_ms: u64,

    /// Transaction timeout (in seconds)
    pub tx_timeout_secs: u64,
}

impl BlockchainConfig {
    /// Create a new configuration for the specified network
    pub fn new(network: Network) -> Self {
        let horizon_url = network.default_horizon_url().to_string();
        let soroban_rpc_url = network.default_soroban_rpc_url().to_string();
        let network_passphrase = network.passphrase().to_string();

        Self {
            network,
            horizon_url,
            soroban_rpc_url,
            network_passphrase,
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_initial_delay_ms: 100,
            retry_max_delay_ms: 5000,
            retry_multiplier: 2.0,
            tx_poll_interval_ms: 1000,
            tx_timeout_secs: 60,
        }
    }

    /// Create configuration for testnet
    pub fn testnet() -> Self {
        Self::new(Network::Testnet)
    }

    /// Create configuration for mainnet
    pub fn mainnet() -> Self {
        Self::new(Network::Mainnet)
    }

    /// Create configuration for futurenet
    pub fn futurenet() -> Self {
        Self::new(Network::Futurenet)
    }

    /// Create a custom configuration
    pub fn custom(
        horizon_url: String,
        soroban_rpc_url: String,
        network_passphrase: String,
    ) -> Result<Self> {
        if horizon_url.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Horizon URL cannot be empty".to_string(),
            ));
        }
        if soroban_rpc_url.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Soroban RPC URL cannot be empty".to_string(),
            ));
        }
        if network_passphrase.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Network passphrase cannot be empty".to_string(),
            ));
        }

        Ok(Self {
            network: Network::Custom,
            horizon_url,
            soroban_rpc_url,
            network_passphrase,
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_initial_delay_ms: 100,
            retry_max_delay_ms: 5000,
            retry_multiplier: 2.0,
            tx_poll_interval_ms: 1000,
            tx_timeout_secs: 60,
        })
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set maximum retries
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set retry delays
    pub fn with_retry_config(
        mut self,
        initial_delay_ms: u64,
        max_delay_ms: u64,
        multiplier: f64,
    ) -> Self {
        self.retry_initial_delay_ms = initial_delay_ms;
        self.retry_max_delay_ms = max_delay_ms;
        self.retry_multiplier = multiplier;
        self
    }

    /// Set transaction polling configuration
    pub fn with_tx_config(mut self, poll_interval_ms: u64, timeout_secs: u64) -> Self {
        self.tx_poll_interval_ms = poll_interval_ms;
        self.tx_timeout_secs = timeout_secs;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.horizon_url.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Horizon URL cannot be empty".to_string(),
            ));
        }
        if self.soroban_rpc_url.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Soroban RPC URL cannot be empty".to_string(),
            ));
        }
        if self.network_passphrase.is_empty() {
            return Err(BlockchainError::ConfigError(
                "Network passphrase cannot be empty".to_string(),
            ));
        }
        if self.max_retries == 0 {
            return Err(BlockchainError::ConfigError(
                "Max retries must be greater than 0".to_string(),
            ));
        }
        if self.retry_initial_delay_ms == 0 {
            return Err(BlockchainError::ConfigError(
                "Retry initial delay must be greater than 0".to_string(),
            ));
        }
        if self.retry_multiplier <= 1.0 {
            return Err(BlockchainError::ConfigError(
                "Retry multiplier must be greater than 1.0".to_string(),
            ));
        }
        if self.tx_poll_interval_ms == 0 {
            return Err(BlockchainError::ConfigError(
                "Transaction poll interval must be greater than 0".to_string(),
            ));
        }
        if self.tx_timeout_secs == 0 {
            return Err(BlockchainError::ConfigError(
                "Transaction timeout must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self::testnet()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_passphrase() {
        assert_eq!(
            Network::Testnet.passphrase(),
            "Test SDF Network ; September 2015"
        );
        assert_eq!(
            Network::Mainnet.passphrase(),
            "Public Global Stellar Network ; September 2015"
        );
    }

    #[test]
    fn test_network_urls() {
        assert_eq!(
            Network::Testnet.default_horizon_url(),
            "https://horizon-testnet.stellar.org"
        );
        assert_eq!(
            Network::Testnet.default_soroban_rpc_url(),
            "https://soroban-testnet.stellar.org"
        );
    }

    #[test]
    fn test_testnet_config() {
        let config = BlockchainConfig::testnet();
        assert_eq!(config.network, Network::Testnet);
        assert_eq!(config.horizon_url, "https://horizon-testnet.stellar.org");
        assert_eq!(
            config.soroban_rpc_url,
            "https://soroban-testnet.stellar.org"
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_mainnet_config() {
        let config = BlockchainConfig::mainnet();
        assert_eq!(config.network, Network::Mainnet);
        assert_eq!(config.horizon_url, "https://horizon.stellar.org");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_custom_config() {
        let config = BlockchainConfig::custom(
            "https://custom-horizon.example.com".to_string(),
            "https://custom-soroban.example.com".to_string(),
            "Custom Network Passphrase".to_string(),
        )
        .unwrap();

        assert_eq!(config.network, Network::Custom);
        assert_eq!(config.horizon_url, "https://custom-horizon.example.com");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_custom_config_empty_urls() {
        let result = BlockchainConfig::custom(
            "".to_string(),
            "https://soroban.example.com".to_string(),
            "Custom".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = BlockchainConfig::testnet()
            .with_request_timeout(Duration::from_secs(60))
            .with_max_retries(5)
            .with_retry_config(200, 10000, 2.5)
            .with_tx_config(2000, 120);

        assert_eq!(config.request_timeout, Duration::from_secs(60));
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_initial_delay_ms, 200);
        assert_eq!(config.retry_max_delay_ms, 10000);
        assert_eq!(config.retry_multiplier, 2.5);
        assert_eq!(config.tx_poll_interval_ms, 2000);
        assert_eq!(config.tx_timeout_secs, 120);
    }

    #[test]
    fn test_config_validation() {
        let mut config = BlockchainConfig::testnet();

        // Valid config
        assert!(config.validate().is_ok());

        // Invalid max retries
        config.max_retries = 0;
        assert!(config.validate().is_err());

        // Invalid retry multiplier
        config.max_retries = 3;
        config.retry_multiplier = 0.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_default_config() {
        let config = BlockchainConfig::default();
        assert_eq!(config.network, Network::Testnet);
        assert!(config.validate().is_ok());
    }
}
