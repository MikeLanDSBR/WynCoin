use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Result, WynError};

use super::Transaction;

pub const ZERO_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHeader {
    pub version: u32,
    pub network_id: String,
    pub prev_hash: String,
    pub merkle_root: String,
    pub timestamp: i64,
    pub difficulty: u32,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub index: u64,
    pub header: BlockHeader,
    pub hash: String,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn genesis(network_id: &str) -> Self {
        let mut block = Self {
            index: 0,
            header: BlockHeader {
                version: 1,
                network_id: network_id.to_string(),
                prev_hash: ZERO_HASH.to_string(),
                merkle_root: ZERO_HASH.to_string(),
                timestamp: 1_753_660_800_000,
                difficulty: 0,
                nonce: 0,
            },
            hash: String::new(),
            transactions: Vec::new(),
        };
        block.hash = block.compute_hash();
        block
    }

    pub fn new(
        network_id: &str,
        index: u64,
        prev_hash: String,
        transactions: Vec<Transaction>,
        difficulty: u32,
    ) -> Result<Self> {
        let merkle_root = Self::compute_merkle_root(&transactions)?;
        let mut block = Self {
            index,
            header: BlockHeader {
                version: 1,
                network_id: network_id.to_string(),
                prev_hash,
                merkle_root,
                timestamp: Utc::now().timestamp_millis(),
                difficulty,
                nonce: 0,
            },
            hash: String::new(),
            transactions,
        };
        block.hash = block.compute_hash();
        Ok(block)
    }

    pub fn compute_hash(&self) -> String {
        let encoded = serde_json::to_vec(&(
            self.index,
            &self.header.version,
            &self.header.network_id,
            &self.header.prev_hash,
            &self.header.merkle_root,
            self.header.timestamp,
            self.header.difficulty,
            self.header.nonce,
        ))
        .expect("serialização do cabeçalho não pode falhar");

        let mut hasher = Sha256::new();
        hasher.update(encoded);
        hex::encode(hasher.finalize())
    }

    pub fn mine(&mut self) -> Result<()> {
        if self.header.difficulty == 0 || self.header.difficulty > 64 {
            return Err(WynError::Validation(
                "dificuldade inválida para Proof of Work".into(),
            ));
        }
        let prefix = "0".repeat(self.header.difficulty as usize);
        loop {
            self.hash = self.compute_hash();
            if self.hash.starts_with(&prefix) {
                return Ok(());
            }
            self.header.nonce = self
                .header
                .nonce
                .checked_add(1)
                .ok_or_else(|| WynError::Validation("nonce esgotado".into()))?;
        }
    }

    pub fn is_valid_pow(&self) -> bool {
        if self.header.difficulty == 0 || self.header.difficulty > 64 {
            return false;
        }
        let prefix = "0".repeat(self.header.difficulty as usize);
        self.hash == self.compute_hash() && self.hash.starts_with(&prefix)
    }

    pub fn has_valid_merkle_root(&self) -> Result<bool> {
        Ok(self.header.merkle_root == Self::compute_merkle_root(&self.transactions)?)
    }

    pub fn has_valid_coinbase_layout(&self) -> bool {
        if self.transactions.is_empty() || !self.transactions[0].is_coinbase {
            return false;
        }
        self.transactions
            .iter()
            .filter(|transaction| transaction.is_coinbase)
            .count()
            == 1
    }

    pub fn compute_merkle_root(transactions: &[Transaction]) -> Result<String> {
        if transactions.is_empty() {
            return Ok(ZERO_HASH.to_string());
        }

        let mut hashes: Vec<String> = transactions
            .iter()
            .map(|transaction| transaction.id.clone())
            .collect();

        while hashes.len() > 1 {
            if hashes.len() % 2 != 0 {
                if let Some(last) = hashes.last().cloned() {
                    hashes.push(last);
                }
            }

            let mut next = Vec::with_capacity(hashes.len() / 2);
            for pair in hashes.chunks_exact(2) {
                let mut hasher = Sha256::new();
                hasher.update(pair[0].as_bytes());
                hasher.update(pair[1].as_bytes());
                next.push(hex::encode(hasher.finalize()));
            }
            hashes = next;
        }

        hashes
            .pop()
            .ok_or_else(|| WynError::Validation("não foi possível calcular Merkle root".into()))
    }
}
