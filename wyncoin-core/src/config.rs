use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Result, WynError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub network: NetworkConfig,
    pub chain: ChainConfig,
    pub mining: MiningConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub id: String,
    pub listen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub difficulty: u32,
    pub block_reward: u64,
    pub min_block_interval_seconds: u64,
    pub max_transactions_per_block: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    pub enabled: bool,
    pub mine_empty_blocks: bool,
    pub interval_seconds: u64,
    pub miner_wallet: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub database: PathBuf,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig {
                id: "wyncoin-local-v1".into(),
                listen: "127.0.0.1:9332".into(),
            },
            chain: ChainConfig {
                difficulty: 4,
                block_reward: 5_000_000_000,
                min_block_interval_seconds: 60,
                max_transactions_per_block: 100,
            },
            mining: MiningConfig {
                enabled: true,
                mine_empty_blocks: true,
                interval_seconds: 60,
                miner_wallet: PathBuf::from("wallets/miner.json"),
            },
            storage: StorageConfig {
                database: PathBuf::from("blockchain.db"),
            },
        }
    }
}

impl NodeConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;
        config.resolve_relative_paths(path);
        config.validate()?;
        Ok(config)
    }

    pub fn write_default(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(&Self::default())
            .map_err(|e| WynError::Config(format!("não foi possível gerar TOML: {e}")))?;
        fs::write(path, body)?;
        Ok(())
    }

    fn resolve_relative_paths(&mut self, config_path: &Path) {
        let base = config_path.parent().unwrap_or_else(|| Path::new("."));
        if self.storage.database.is_relative() {
            self.storage.database = base.join(&self.storage.database);
        }
        if self.mining.miner_wallet.is_relative() {
            self.mining.miner_wallet = base.join(&self.mining.miner_wallet);
        }
    }

    pub fn validate(&self) -> Result<()> {
        let addr: SocketAddr = self
            .network
            .listen
            .parse()
            .map_err(|_| WynError::Config("network.listen não é um endereço válido".into()))?;

        if !addr.ip().is_loopback() {
            return Err(WynError::Config(
                "a versão 0.1.0 só permite escutar em 127.0.0.1/::1".into(),
            ));
        }
        if self.network.id.trim().is_empty() {
            return Err(WynError::Config("network.id não pode ficar vazio".into()));
        }
        if !(1..=8).contains(&self.chain.difficulty) {
            return Err(WynError::Config(
                "chain.difficulty deve estar entre 1 e 8".into(),
            ));
        }
        if self.chain.block_reward == 0 {
            return Err(WynError::Config("block_reward deve ser maior que zero".into()));
        }
        if !(1..=86_400).contains(&self.chain.min_block_interval_seconds) {
            return Err(WynError::Config(
                "chain.min_block_interval_seconds deve estar entre 1 e 86400".into(),
            ));
        }
        if self.chain.max_transactions_per_block == 0 {
            return Err(WynError::Config(
                "max_transactions_per_block deve ser maior que zero".into(),
            ));
        }
        Ok(())
    }
}
