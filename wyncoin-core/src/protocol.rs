use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blockchain::Transaction;
use crate::{Result, WynError};

pub const MAX_REQUEST_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    Balance { address: String },
    Utxos { address: String },
    SubmitTransaction { transaction: Transaction },
    Mine { blocks: u32 },
    Blocks { limit: usize },
    Block { height: u64 },
    Mempool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ApiResponse {
    pub fn success<T: Serialize>(value: T) -> Self {
        match serde_json::to_value(value) {
            Ok(data) => Self {
                ok: true,
                data: Some(data),
                error: None,
            },
            Err(error) => Self::failure(format!("falha ao serializar resposta: {error}")),
        }
    }

    pub fn empty_success() -> Self {
        Self {
            ok: true,
            data: None,
            error: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }

    pub fn require_data<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        if !self.ok {
            return Err(WynError::Protocol(
                self.error.clone().unwrap_or_else(|| "erro desconhecido".into()),
            ));
        }
        let data = self
            .data
            .clone()
            .ok_or_else(|| WynError::Protocol("resposta sem dados".into()))?;
        Ok(serde_json::from_value(data)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub version: String,
    pub network_id: String,
    pub height: u64,
    pub tip_hash: String,
    pub difficulty: u32,
    pub block_reward: u64,
    pub mempool_size: usize,
    pub mining_enabled: bool,
    pub miner_address: String,
    pub database: String,
    pub uptime_seconds: u64,
}

pub fn send_request(address: &str, request: &Request) -> Result<ApiResponse> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut encoded = serde_json::to_vec(request)?;
    if encoded.len() > MAX_REQUEST_BYTES {
        return Err(WynError::Protocol("requisição excede 1 MiB".into()));
    }
    encoded.push(b'\n');
    stream.write_all(&encoded)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Write);

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.len() > MAX_REQUEST_BYTES {
        return Err(WynError::Protocol("resposta excede 1 MiB".into()));
    }
    if response.trim().is_empty() {
        return Err(WynError::Protocol("o nó encerrou sem responder".into()));
    }
    Ok(serde_json::from_str(&response)?)
}
