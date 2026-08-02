use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Result, WynError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub network: NetworkConfig,
    #[serde(default)]
    pub p2p: P2pConfig,
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
pub struct P2pConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_p2p_listen")]
    pub listen: String,
    #[serde(default)]
    pub advertise: Option<String>,
    #[serde(default)]
    pub seeds: Vec<String>,
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,
}

fn default_p2p_listen() -> String {
    "127.0.0.1:9333".into()
}
fn default_max_peers() -> usize {
    32
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default_p2p_listen(),
            advertise: None,
            seeds: Vec::new(),
            max_peers: default_max_peers(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub consensus_version: u32,
    /// Alvo PoW inicial sobre os primeiros 64 bits do hash. Menor é mais difícil.
    pub initial_target: u64,
    pub block_reward: u64,
    pub target_block_time_seconds: u64,
    pub retarget_interval_blocks: u64,
    pub max_retarget_factor: u64,
    pub max_transactions_per_block: usize,
    pub max_block_size_bytes: usize,
    pub coinbase_maturity_blocks: u64,
    pub max_future_block_time_seconds: u64,
    pub upgrade_vote_window_blocks: u64,
    pub upgrade_vote_threshold_percent: u8,
    pub upgrade_activation_delay_blocks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    pub enabled: bool,
    pub mine_empty_blocks: bool,
    pub interval_seconds: u64,
    /// Endereço público que recebe a coinbase. Não exige que o daemon guarde a chave.
    #[serde(default)]
    pub miner_address: Option<String>,
    /// Compatibilidade exclusiva com instalações privadas anteriores a P2P.
    #[serde(default)]
    pub miner_wallet: Option<PathBuf>,
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
            p2p: P2pConfig::default(),
            chain: ChainConfig {
                consensus_version: 4,
                initial_target: 1_099_511_627_775,
                block_reward: 1_000_000_000,
                target_block_time_seconds: 60,
                retarget_interval_blocks: 20,
                max_retarget_factor: 4,
                max_transactions_per_block: 100,
                max_block_size_bytes: 2 * 1024 * 1024,
                coinbase_maturity_blocks: 20,
                max_future_block_time_seconds: 30,
                upgrade_vote_window_blocks: 200,
                upgrade_vote_threshold_percent: 90,
                upgrade_activation_delay_blocks: 100,
            },
            mining: MiningConfig {
                enabled: true,
                mine_empty_blocks: true,
                interval_seconds: 60,
                miner_address: None,
                miner_wallet: Some(PathBuf::from("wallets/miner.json")),
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
        if let Some(miner_wallet) = &mut self.mining.miner_wallet {
            if miner_wallet.is_relative() {
                *miner_wallet = base.join(&miner_wallet);
            }
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
        if self.p2p.enabled {
            self.p2p
                .listen
                .parse::<SocketAddr>()
                .map_err(|_| WynError::Config("p2p.listen não é um endereço válido".into()))?;
            if let Some(advertise) = &self.p2p.advertise {
                advertise.parse::<SocketAddr>().map_err(|_| {
                    WynError::Config("p2p.advertise não é um endereço válido".into())
                })?;
            }
            if self.p2p.max_peers == 0 || self.p2p.max_peers > 128 {
                return Err(WynError::Config(
                    "p2p.max_peers deve estar entre 1 e 128".into(),
                ));
            }
            for seed in &self.p2p.seeds {
                seed.parse::<SocketAddr>()
                    .map_err(|_| WynError::Config(format!("seed P2P inválido: {seed}")))?;
            }
        }
        if self.chain.initial_target == 0 {
            return Err(WynError::Config(
                "chain.initial_target deve ser maior que zero".into(),
            ));
        }
        if self.chain.consensus_version != 4 {
            return Err(WynError::Config(
                "esta versão do nó exige chain.consensus_version = 4".into(),
            ));
        }
        if self.chain.block_reward == 0 {
            return Err(WynError::Config(
                "block_reward deve ser maior que zero".into(),
            ));
        }
        if !(1..=86_400).contains(&self.chain.target_block_time_seconds) {
            return Err(WynError::Config(
                "chain.target_block_time_seconds deve estar entre 1 e 86400".into(),
            ));
        }
        if !(2..=10_000).contains(&self.chain.retarget_interval_blocks) {
            return Err(WynError::Config(
                "chain.retarget_interval_blocks deve estar entre 2 e 10000".into(),
            ));
        }
        if !(2..=16).contains(&self.chain.max_retarget_factor) {
            return Err(WynError::Config(
                "chain.max_retarget_factor deve estar entre 2 e 16".into(),
            ));
        }
        if self.chain.max_transactions_per_block == 0 {
            return Err(WynError::Config(
                "max_transactions_per_block deve ser maior que zero".into(),
            ));
        }
        if !(1_024..=16 * 1024 * 1024).contains(&self.chain.max_block_size_bytes) {
            return Err(WynError::Config(
                "chain.max_block_size_bytes deve estar entre 1024 e 16777216".into(),
            ));
        }
        if self.chain.coinbase_maturity_blocks == 0 {
            return Err(WynError::Config(
                "chain.coinbase_maturity_blocks deve ser maior que zero".into(),
            ));
        }
        if !(1..=600).contains(&self.chain.max_future_block_time_seconds) {
            return Err(WynError::Config(
                "chain.max_future_block_time_seconds deve estar entre 1 e 600".into(),
            ));
        }
        if !(10..=10_000).contains(&self.chain.upgrade_vote_window_blocks) {
            return Err(WynError::Config(
                "chain.upgrade_vote_window_blocks deve estar entre 10 e 10000".into(),
            ));
        }
        if !(51..=100).contains(&self.chain.upgrade_vote_threshold_percent) {
            return Err(WynError::Config(
                "chain.upgrade_vote_threshold_percent deve estar entre 51 e 100".into(),
            ));
        }
        if self.mining.enabled
            && self
                .mining
                .miner_address
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            && self.mining.miner_wallet.is_none()
        {
            return Err(WynError::Config(
                "mining exige miner_address ou miner_wallet".into(),
            ));
        }
        Ok(())
    }
}
