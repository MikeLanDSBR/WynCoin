use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Result, WynError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxInput {
    pub tx_id: String,
    pub output_index: usize,
    pub signature: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxOutput {
    pub recipient: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub version: u32,
    pub id: String,
    pub timestamp: i64,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub is_coinbase: bool,
    pub coinbase_data: Option<String>,
}

#[derive(Serialize)]
struct SigningInput<'a> {
    tx_id: &'a str,
    output_index: usize,
    public_key: &'a str,
}

#[derive(Serialize)]
struct SigningPayload<'a> {
    version: u32,
    timestamp: i64,
    inputs: Vec<SigningInput<'a>>,
    outputs: &'a [TxOutput],
    is_coinbase: bool,
    coinbase_data: &'a Option<String>,
}

#[derive(Serialize)]
struct IdPayload<'a> {
    version: u32,
    timestamp: i64,
    inputs: &'a [TxInput],
    outputs: &'a [TxOutput],
    is_coinbase: bool,
    coinbase_data: &'a Option<String>,
}

impl Transaction {
    pub fn new(inputs: Vec<TxInput>, outputs: Vec<TxOutput>) -> Result<Self> {
        let mut transaction = Self {
            version: 1,
            id: String::new(),
            timestamp: Utc::now().timestamp_millis(),
            inputs,
            outputs,
            is_coinbase: false,
            coinbase_data: None,
        };
        transaction.refresh_id()?;
        Ok(transaction)
    }

    pub fn new_coinbase(miner_address: &str, reward: u64, height: u64) -> Result<Self> {
        let mut transaction = Self {
            version: 1,
            id: String::new(),
            timestamp: Utc::now().timestamp_millis(),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                recipient: miner_address.to_string(),
                amount: reward,
            }],
            is_coinbase: true,
            coinbase_data: Some(format!("wyncoin-v1-height-{height}")),
        };
        transaction.refresh_id()?;
        Ok(transaction)
    }

    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        let inputs = self
            .inputs
            .iter()
            .map(|input| SigningInput {
                tx_id: &input.tx_id,
                output_index: input.output_index,
                public_key: &input.public_key,
            })
            .collect();

        Ok(serde_json::to_vec(&SigningPayload {
            version: self.version,
            timestamp: self.timestamp,
            inputs,
            outputs: &self.outputs,
            is_coinbase: self.is_coinbase,
            coinbase_data: &self.coinbase_data,
        })?)
    }

    pub fn refresh_id(&mut self) -> Result<()> {
        self.id = Self::hash_bytes(&self.id_bytes()?);
        Ok(())
    }

    pub fn has_valid_id(&self) -> Result<bool> {
        Ok(self.id == Self::hash_bytes(&self.id_bytes()?))
    }

    pub fn checked_total_output(&self) -> Result<u64> {
        self.outputs.iter().try_fold(0u64, |total, output| {
            total
                .checked_add(output.amount)
                .ok_or_else(|| WynError::Validation("overflow na soma dos outputs".into()))
        })
    }

    fn id_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&IdPayload {
            version: self.version,
            timestamp: self.timestamp,
            inputs: &self.inputs,
            outputs: &self.outputs,
            is_coinbase: self.is_coinbase,
            coinbase_data: &self.coinbase_data,
        })?)
    }

    fn hash_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }
}

impl std::fmt::Display for Transaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.is_coinbase {
            "coinbase"
        } else {
            "regular"
        };
        write!(
            formatter,
            "TX {} ({kind}, {} input(s), {} output(s))",
            self.id.get(..12).unwrap_or(&self.id),
            self.inputs.len(),
            self.outputs.len()
        )
    }
}
