pub mod consensus;
pub mod network;
pub mod node;
pub mod support;
pub mod wallet;

/// Caminho público legado. Mantido para não quebrar os binários atuais nem
/// consumidores externos enquanto a nomenclatura migra para `consensus`.
pub mod blockchain {
    pub use crate::consensus::*;
}

/// Caminho público legado para os tipos de API e P2P.
pub mod protocol {
    pub use crate::network::*;
}

pub use consensus::{Block, Blockchain, ChainParams, Transaction, TxInput, TxOutput, Utxo};
pub use network::{send_request, ApiResponse, NodeStatus, P2pMessage, PeerHello, Request};
pub use node::{NodeConfig, Storage};
pub use support::{format_wyn, parse_wyn, SATOSHIS_PER_WYN, WynError};
pub use wallet::Wallet;

pub type Result<T> = std::result::Result<T, WynError>;
