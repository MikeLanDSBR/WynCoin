use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::blockchain::{Block, Transaction};
use crate::Result;

#[derive(Debug, Clone)]
pub struct Storage {
    path: PathBuf,
}

impl Storage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = self.open()?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS blocks (
                height INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                previous_hash TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mempool (
                txid TEXT PRIMARY KEY,
                received_at INTEGER NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS peers (
                address TEXT PRIMARY KEY,
                last_seen INTEGER NOT NULL
            );

            INSERT INTO metadata(key, value)
            VALUES ('schema_version', '1')
            ON CONFLICT(key) DO NOTHING;
            "#,
        )?;
        Ok(())
    }

    pub fn load_blocks(&self) -> Result<Vec<Block>> {
        let connection = self.open()?;
        let mut statement = connection.prepare("SELECT data FROM blocks ORDER BY height ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut blocks = Vec::new();
        for row in rows {
            blocks.push(serde_json::from_str(&row?)?);
        }
        Ok(blocks)
    }

    pub fn load_mempool(&self) -> Result<Vec<Transaction>> {
        let connection = self.open()?;
        let mut statement =
            connection.prepare("SELECT data FROM mempool ORDER BY received_at ASC, txid ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut transactions = Vec::new();
        for row in rows {
            transactions.push(serde_json::from_str(&row?)?);
        }
        Ok(transactions)
    }

    pub fn insert_genesis_if_empty(&self, genesis: &Block) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT OR IGNORE INTO blocks(height, hash, previous_hash, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                genesis.index,
                &genesis.hash,
                &genesis.header.prev_hash,
                serde_json::to_string(genesis)?
            ],
        )?;
        Ok(())
    }

    pub fn insert_mempool_transaction(&self, transaction: &Transaction) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO mempool(txid, received_at, data) VALUES (?1, ?2, ?3)",
            params![
                &transaction.id,
                transaction.timestamp,
                serde_json::to_string(transaction)?
            ],
        )?;
        Ok(())
    }

    pub fn remove_mempool_transaction(&self, transaction_id: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute("DELETE FROM mempool WHERE txid = ?1", [transaction_id])?;
        Ok(())
    }

    pub fn append_block(&self, block: &Block) -> Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO blocks(height, hash, previous_hash, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                block.index,
                &block.hash,
                &block.header.prev_hash,
                serde_json::to_string(block)?
            ],
        )?;

        for confirmed in block.transactions.iter().skip(1) {
            transaction.execute(
                "DELETE FROM mempool WHERE txid = ?1",
                params![&confirmed.id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_chain(&self, blocks: &[Block], mempool: &[Transaction]) -> Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM blocks", [])?;
        for block in blocks {
            transaction.execute(
                "INSERT INTO blocks(height, hash, previous_hash, data) VALUES (?1, ?2, ?3, ?4)",
                params![
                    block.index,
                    &block.hash,
                    &block.header.prev_hash,
                    serde_json::to_string(block)?
                ],
            )?;
        }
        transaction.execute("DELETE FROM mempool", [])?;
        for item in mempool {
            transaction.execute(
                "INSERT INTO mempool(txid, received_at, data) VALUES (?1, ?2, ?3)",
                params![&item.id, item.timestamp, serde_json::to_string(item)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_peers(&self, limit: usize) -> Result<Vec<String>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT address FROM peers ORDER BY last_seen DESC, address ASC LIMIT ?1")?;
        let rows = statement.query_map([limit as i64], |row| row.get::<_, String>(0))?;
        let mut peers = Vec::new();
        for row in rows {
            peers.push(row?);
        }
        Ok(peers)
    }

    pub fn remember_peer(&self, address: &str, last_seen: i64) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO peers(address, last_seen) VALUES (?1, ?2) ON CONFLICT(address) DO UPDATE SET last_seen = excluded.last_seen",
            params![address, last_seen],
        )?;
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}
