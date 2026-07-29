use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::wallet::Wallet;
use crate::{Result, WynError};

use super::block::Block;
use super::{Transaction, TxOutput};

pub type UtxoKey = (String, usize);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Utxo {
    pub tx_id: String,
    pub output_index: usize,
    pub output: TxOutput,
}

#[derive(Debug, Clone)]
pub struct ChainParams {
    pub network_id: String,
    pub difficulty: u32,
    pub block_reward: u64,
    pub min_block_interval_seconds: u64,
    pub max_transactions_per_block: usize,
}

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub mempool: Vec<Transaction>,
    pub params: ChainParams,
    utxo_set: HashMap<UtxoKey, TxOutput>,
}

impl Blockchain {
    pub fn new(params: ChainParams) -> Self {
        let genesis = Block::genesis(&params.network_id);
        Self {
            chain: vec![genesis],
            mempool: Vec::new(),
            params,
            utxo_set: HashMap::new(),
        }
    }

    pub fn from_persisted(
        params: ChainParams,
        blocks: Vec<Block>,
        persisted_mempool: Vec<Transaction>,
    ) -> Result<Self> {
        let chain = if blocks.is_empty() {
            vec![Block::genesis(&params.network_id)]
        } else {
            blocks
        };

        let utxo_set = Self::validate_chain_and_build_utxo(&params, &chain)?;
        let mut blockchain = Self {
            chain,
            mempool: Vec::new(),
            params,
            utxo_set,
        };

        for transaction in persisted_mempool {
            blockchain.add_to_mempool(transaction)?;
        }
        Ok(blockchain)
    }

    pub fn last_block(&self) -> &Block {
        self.chain.last().expect("a blockchain nunca fica vazia")
    }

    pub fn height(&self) -> u64 {
        self.last_block().index
    }

    pub fn balance_of(&self, address: &str) -> Result<u64> {
        self.get_utxos_for(address)
            .iter()
            .try_fold(0u64, |total, utxo| {
                total
                    .checked_add(utxo.output.amount)
                    .ok_or_else(|| WynError::Validation("overflow ao calcular saldo".into()))
            })
    }

    pub fn get_utxos_for(&self, address: &str) -> Vec<Utxo> {
        let mut items: Vec<Utxo> = self
            .utxo_set
            .iter()
            .filter(|(_, output)| output.recipient == address)
            .map(|((tx_id, output_index), output)| Utxo {
                tx_id: tx_id.clone(),
                output_index: *output_index,
                output: output.clone(),
            })
            .collect();
        items.sort_by(|left, right| {
            left.tx_id
                .cmp(&right.tx_id)
                .then(left.output_index.cmp(&right.output_index))
        });
        items
    }

    pub fn add_to_mempool(&mut self, transaction: Transaction) -> Result<u64> {
        if self.mempool.iter().any(|item| item.id == transaction.id) {
            return Err(WynError::Validation("transação já está no mempool".into()));
        }

        let reserved = self.mempool_spent_outputs();
        let fee = Self::validate_regular_transaction(&transaction, &self.utxo_set, &reserved)?;
        self.mempool.push(transaction);
        Ok(fee)
    }

    pub fn remove_from_mempool(&mut self, transaction_id: &str) {
        self.mempool.retain(|item| item.id != transaction_id);
    }

    pub fn build_candidate_block(&self, miner_address: &str) -> Result<Block> {
        if miner_address.trim().is_empty() {
            return Err(WynError::Validation(
                "endereço do minerador não pode ficar vazio".into(),
            ));
        }

        let mut temp_utxo = self.utxo_set.clone();
        let mut selected = Vec::new();
        let mut fees = 0u64;

        for transaction in self
            .mempool
            .iter()
            .take(self.params.max_transactions_per_block)
        {
            let fee = Self::validate_regular_transaction(
                transaction,
                &temp_utxo,
                &HashSet::new(),
            )?;
            Self::apply_regular_transaction(transaction, &mut temp_utxo);
            fees = fees
                .checked_add(fee)
                .ok_or_else(|| WynError::Validation("overflow nas taxas do bloco".into()))?;
            selected.push(transaction.clone());
        }

        let reward = self
            .params
            .block_reward
            .checked_add(fees)
            .ok_or_else(|| WynError::Validation("overflow na recompensa do bloco".into()))?;

        let height = self.height() + 1;
        let coinbase = Transaction::new_coinbase(miner_address, reward, height)?;
        let mut transactions = Vec::with_capacity(selected.len() + 1);
        transactions.push(coinbase);
        transactions.extend(selected);

        Block::new(
            &self.params.network_id,
            height,
            self.last_block().hash.clone(),
            transactions,
            self.params.difficulty,
        )
    }

    pub fn commit_block(&mut self, block: Block) -> Result<()> {
        let next_utxo = self.validate_next_block(&block)?;
        let confirmed: HashSet<&str> = block
            .transactions
            .iter()
            .skip(1)
            .map(|transaction| transaction.id.as_str())
            .collect();
        self.mempool
            .retain(|transaction| !confirmed.contains(transaction.id.as_str()));
        self.utxo_set = next_utxo;
        self.chain.push(block);
        Ok(())
    }

    pub fn mine_and_commit(&mut self, miner_address: &str) -> Result<Block> {
        let mut block = self.build_candidate_block(miner_address)?;
        block.mine()?;
        self.commit_block(block.clone())?;
        Ok(block)
    }

    pub fn validate_full_chain(&self) -> Result<()> {
        Self::validate_chain_and_build_utxo(&self.params, &self.chain)?;
        Ok(())
    }

    fn validate_next_block(&self, block: &Block) -> Result<HashMap<UtxoKey, TxOutput>> {
        let previous = self.last_block();
        if block.index != previous.index + 1 {
            return Err(WynError::Validation("altura de bloco inválida".into()));
        }
        if block.header.prev_hash != previous.hash {
            return Err(WynError::Validation("hash anterior inválido".into()));
        }
        if block.header.timestamp < previous.header.timestamp {
            return Err(WynError::Validation("timestamp anterior ao bloco precedente".into()));
        }
        Self::validate_min_block_interval(&self.params, previous, block)?;

        let mut utxo = self.utxo_set.clone();
        Self::validate_and_apply_non_genesis_block(&self.params, block, &mut utxo)?;
        Ok(utxo)
    }

    fn validate_chain_and_build_utxo(
        params: &ChainParams,
        chain: &[Block],
    ) -> Result<HashMap<UtxoKey, TxOutput>> {
        if chain.is_empty() {
            return Err(WynError::Validation("blockchain vazia".into()));
        }

        let expected_genesis = Block::genesis(&params.network_id);
        if chain[0] != expected_genesis {
            return Err(WynError::Validation(
                "bloco gênesis não pertence a esta rede".into(),
            ));
        }

        let mut utxo = HashMap::new();
        for index in 1..chain.len() {
            let previous = &chain[index - 1];
            let block = &chain[index];
            if block.index != index as u64 {
                return Err(WynError::Validation(format!(
                    "altura inválida no bloco {}",
                    block.index
                )));
            }
            if block.header.prev_hash != previous.hash {
                return Err(WynError::Validation(format!(
                    "encadeamento inválido no bloco {}",
                    block.index
                )));
            }
            if block.header.timestamp < previous.header.timestamp {
                return Err(WynError::Validation(format!(
                    "timestamp inválido no bloco {}",
                    block.index
                )));
            }
            Self::validate_min_block_interval(params, previous, block)?;
            Self::validate_and_apply_non_genesis_block(params, block, &mut utxo)?;
        }
        Ok(utxo)
    }

    fn validate_min_block_interval(
        params: &ChainParams,
        previous: &Block,
        block: &Block,
    ) -> Result<()> {
        let interval_ms = i64::try_from(params.min_block_interval_seconds)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1_000))
            .ok_or_else(|| WynError::Validation("intervalo mínimo de bloco inválido".into()))?;
        let earliest_timestamp = previous
            .header
            .timestamp
            .checked_add(interval_ms)
            .ok_or_else(|| WynError::Validation("timestamp mínimo de bloco excedido".into()))?;
        if block.header.timestamp < earliest_timestamp {
            return Err(WynError::Validation(format!(
                "bloco {} confirmado antes do intervalo mínimo de {} segundos",
                block.index, params.min_block_interval_seconds
            )));
        }
        Ok(())
    }

    fn validate_and_apply_non_genesis_block(
        params: &ChainParams,
        block: &Block,
        utxo: &mut HashMap<UtxoKey, TxOutput>,
    ) -> Result<()> {
        if block.header.network_id != params.network_id {
            return Err(WynError::Validation(format!(
                "network_id inválido no bloco {}",
                block.index
            )));
        }
        if block.header.difficulty != params.difficulty {
            return Err(WynError::Validation(format!(
                "dificuldade inválida no bloco {}",
                block.index
            )));
        }
        if !block.is_valid_pow() {
            return Err(WynError::Validation(format!(
                "Proof of Work inválido no bloco {}",
                block.index
            )));
        }
        if !block.has_valid_merkle_root()? {
            return Err(WynError::Validation(format!(
                "Merkle root inválida no bloco {}",
                block.index
            )));
        }
        if !block.has_valid_coinbase_layout() {
            return Err(WynError::Validation(format!(
                "coinbase inválida no bloco {}",
                block.index
            )));
        }
        if block.transactions.len() > params.max_transactions_per_block + 1 {
            return Err(WynError::Validation(format!(
                "bloco {} excede o limite de transações",
                block.index
            )));
        }

        let mut working = utxo.clone();
        let mut fees = 0u64;
        for transaction in block.transactions.iter().skip(1) {
            let fee = Self::validate_regular_transaction(
                transaction,
                &working,
                &HashSet::new(),
            )?;
            Self::apply_regular_transaction(transaction, &mut working);
            fees = fees
                .checked_add(fee)
                .ok_or_else(|| WynError::Validation("overflow nas taxas".into()))?;
        }

        let coinbase = &block.transactions[0];
        Self::validate_coinbase(coinbase, block.index, params.block_reward, fees)?;
        Self::apply_coinbase(coinbase, &mut working);
        *utxo = working;
        Ok(())
    }

    fn validate_coinbase(
        transaction: &Transaction,
        height: u64,
        block_reward: u64,
        fees: u64,
    ) -> Result<()> {
        if !transaction.is_coinbase
            || !transaction.inputs.is_empty()
            || transaction.outputs.len() != 1
        {
            return Err(WynError::Validation("estrutura da coinbase inválida".into()));
        }
        if !transaction.has_valid_id()? {
            return Err(WynError::Validation("ID da coinbase inválido".into()));
        }
        let expected_data = format!("wyncoin-v1-height-{height}");
        if transaction.coinbase_data.as_deref() != Some(expected_data.as_str()) {
            return Err(WynError::Validation("dados da coinbase inválidos".into()));
        }
        let expected = block_reward
            .checked_add(fees)
            .ok_or_else(|| WynError::Validation("overflow na coinbase".into()))?;
        let output = &transaction.outputs[0];
        if output.recipient.trim().is_empty() || output.amount != expected {
            return Err(WynError::Validation("recompensa da coinbase inválida".into()));
        }
        Ok(())
    }

    fn validate_regular_transaction(
        transaction: &Transaction,
        utxo: &HashMap<UtxoKey, TxOutput>,
        reserved: &HashSet<UtxoKey>,
    ) -> Result<u64> {
        if transaction.is_coinbase {
            return Err(WynError::Validation(
                "coinbase não pode entrar como transação regular".into(),
            ));
        }
        if !transaction.has_valid_id()? {
            return Err(WynError::Validation("ID da transação inválido".into()));
        }
        if transaction.inputs.is_empty() || transaction.outputs.is_empty() {
            return Err(WynError::Validation(
                "transação precisa de inputs e outputs".into(),
            ));
        }
        if transaction
            .outputs
            .iter()
            .any(|output| output.amount == 0 || output.recipient.trim().is_empty())
        {
            return Err(WynError::Validation("output inválido".into()));
        }

        let signing_bytes = transaction.signing_bytes()?;
        let mut seen = HashSet::new();
        let mut input_total = 0u64;

        for input in &transaction.inputs {
            let key = (input.tx_id.clone(), input.output_index);
            if !seen.insert(key.clone()) {
                return Err(WynError::Validation(
                    "o mesmo UTXO aparece duas vezes na transação".into(),
                ));
            }
            if reserved.contains(&key) {
                return Err(WynError::Validation(
                    "UTXO já reservado por outra transação do mempool".into(),
                ));
            }

            let previous_output = utxo
                .get(&key)
                .ok_or_else(|| WynError::Validation("UTXO inexistente ou já gasto".into()))?;
            let public_pem = Wallet::public_key_pem_from_b64(&input.public_key)
                .ok_or_else(|| WynError::Validation("chave pública inválida".into()))?;
            let derived_address = Wallet::address_from_public_pem(&public_pem);
            if derived_address != previous_output.recipient {
                return Err(WynError::Validation(
                    "a chave pública não pertence ao UTXO".into(),
                ));
            }
            if !Wallet::verify(&public_pem, &signing_bytes, &input.signature) {
                return Err(WynError::Validation("assinatura inválida".into()));
            }
            input_total = input_total
                .checked_add(previous_output.amount)
                .ok_or_else(|| WynError::Validation("overflow nos inputs".into()))?;
        }

        let output_total = transaction.checked_total_output()?;
        input_total
            .checked_sub(output_total)
            .ok_or_else(|| WynError::Validation("saldo insuficiente".into()))
    }

    fn apply_regular_transaction(
        transaction: &Transaction,
        utxo: &mut HashMap<UtxoKey, TxOutput>,
    ) {
        for input in &transaction.inputs {
            utxo.remove(&(input.tx_id.clone(), input.output_index));
        }
        for (output_index, output) in transaction.outputs.iter().enumerate() {
            utxo.insert((transaction.id.clone(), output_index), output.clone());
        }
    }

    fn apply_coinbase(transaction: &Transaction, utxo: &mut HashMap<UtxoKey, TxOutput>) {
        for (output_index, output) in transaction.outputs.iter().enumerate() {
            utxo.insert((transaction.id.clone(), output_index), output.clone());
        }
    }

    fn mempool_spent_outputs(&self) -> HashSet<UtxoKey> {
        self.mempool
            .iter()
            .flat_map(|transaction| {
                transaction
                    .inputs
                    .iter()
                    .map(|input| (input.tx_id.clone(), input.output_index))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::Wallet;

    fn params() -> ChainParams {
        ChainParams {
            network_id: "test-network".into(),
            difficulty: 1,
            block_reward: 5_000_000_000,
            min_block_interval_seconds: 0,
            max_transactions_per_block: 100,
        }
    }

    #[test]
    fn mines_and_validates_chain() {
        let wallet = Wallet::generate().unwrap();
        let mut blockchain = Blockchain::new(params());
        blockchain.mine_and_commit(&wallet.address).unwrap();
        blockchain.validate_full_chain().unwrap();
        assert_eq!(blockchain.balance_of(&wallet.address).unwrap(), 5_000_000_000);
    }


    #[test]
    fn signs_spends_and_confirms_transaction() {
        let miner = Wallet::generate().unwrap();
        let recipient = Wallet::generate().unwrap();
        let mut blockchain = Blockchain::new(params());
        blockchain.mine_and_commit(&miner.address).unwrap();

        let transaction = miner
            .build_transaction(
                &blockchain.get_utxos_for(&miner.address),
                &recipient.address,
                1_000_000_000,
                100_000,
            )
            .unwrap();
        blockchain.add_to_mempool(transaction).unwrap();
        blockchain.mine_and_commit(&miner.address).unwrap();
        blockchain.validate_full_chain().unwrap();

        assert_eq!(
            blockchain.balance_of(&recipient.address).unwrap(),
            1_000_000_000
        );
        assert_eq!(
            blockchain.balance_of(&miner.address).unwrap(),
            9_000_000_000
        );
    }

    #[test]
    fn rejects_block_before_minimum_interval() {
        let wallet = Wallet::generate().unwrap();
        let mut params = params();
        params.min_block_interval_seconds = 60;
        let mut blockchain = Blockchain::new(params);
        blockchain.mine_and_commit(&wallet.address).unwrap();

        let mut candidate = blockchain.build_candidate_block(&wallet.address).unwrap();
        candidate.mine().unwrap();
        let error = blockchain.commit_block(candidate).unwrap_err();

        assert!(error
            .to_string()
            .contains("confirmado antes do intervalo mínimo"));
    }
}
