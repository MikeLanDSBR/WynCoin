use std::collections::{HashMap, HashSet};

use chrono::Utc;

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
    pub created_height: u64,
    pub is_coinbase: bool,
}

#[derive(Debug, Clone)]
struct UtxoEntry {
    output: TxOutput,
    created_height: u64,
    is_coinbase: bool,
}

#[derive(Debug, Clone)]
pub struct ChainParams {
    pub network_id: String,
    pub consensus_version: u32,
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

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub mempool: Vec<Transaction>,
    pub params: ChainParams,
    utxo_set: HashMap<UtxoKey, UtxoEntry>,
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
            .filter(|(_, entry)| entry.output.recipient == address)
            .filter(|(_, entry)| Self::is_spendable_at(&self.params, entry, self.height() + 1))
            .map(|((tx_id, output_index), entry)| Utxo {
                tx_id: tx_id.clone(),
                output_index: *output_index,
                output: entry.output.clone(),
                created_height: entry.created_height,
                is_coinbase: entry.is_coinbase,
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
        let fee = Self::validate_regular_transaction(
            &self.params,
            self.height() + 1,
            &transaction,
            &self.utxo_set,
            &reserved,
        )?;
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
                &self.params,
                self.height() + 1,
                transaction,
                &temp_utxo,
                &HashSet::new(),
            )?;
            let next_fees = fees
                .checked_add(fee)
                .ok_or_else(|| WynError::Validation("overflow nas taxas do bloco".into()))?;
            let mut candidate_transactions = Vec::with_capacity(selected.len() + 2);
            candidate_transactions.push(Transaction::new_coinbase(
                miner_address,
                self.params
                    .block_reward
                    .checked_add(next_fees)
                    .ok_or_else(|| {
                        WynError::Validation("overflow na recompensa do bloco".into())
                    })?,
                self.height() + 1,
            )?);
            candidate_transactions.extend(selected.iter().cloned());
            candidate_transactions.push(transaction.clone());
            let candidate = Block::new(
                &self.params.network_id,
                self.params.consensus_version,
                0,
                self.height() + 1,
                self.last_block().hash.clone(),
                candidate_transactions,
                self.next_target(),
            )?;
            if serde_json::to_vec(&candidate)?.len() > self.params.max_block_size_bytes {
                break;
            }
            Self::apply_regular_transaction(transaction, &mut temp_utxo, self.height() + 1);
            fees = next_fees;
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

        let block = Block::new(
            &self.params.network_id,
            self.params.consensus_version,
            0,
            height,
            self.last_block().hash.clone(),
            transactions,
            self.next_target(),
        )?;
        if serde_json::to_vec(&block)?.len() > self.params.max_block_size_bytes {
            return Err(WynError::Validation(
                "coinbase excede o tamanho máximo do bloco".into(),
            ));
        }
        Ok(block)
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

    /// Identifica tanto o gênesis quanto as regras que definem consenso.
    /// Nós com parâmetros diferentes não podem compartilhar blocos.
    pub fn chain_id(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.chain[0].hash.as_bytes());
        hasher.update(self.params.network_id.as_bytes());
        hasher.update(self.params.consensus_version.to_le_bytes());
        hasher.update(self.params.initial_target.to_le_bytes());
        hasher.update(self.params.block_reward.to_le_bytes());
        hasher.update(self.params.target_block_time_seconds.to_le_bytes());
        hasher.update(self.params.retarget_interval_blocks.to_le_bytes());
        hasher.update(self.params.max_retarget_factor.to_le_bytes());
        hasher.update((self.params.max_transactions_per_block as u64).to_le_bytes());
        hasher.update((self.params.max_block_size_bytes as u64).to_le_bytes());
        hasher.update(self.params.coinbase_maturity_blocks.to_le_bytes());
        hasher.update(self.params.max_future_block_time_seconds.to_le_bytes());
        hasher.update(self.params.upgrade_vote_window_blocks.to_le_bytes());
        hasher.update([self.params.upgrade_vote_threshold_percent]);
        hasher.update(self.params.upgrade_activation_delay_blocks.to_le_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn cumulative_work(&self) -> u128 {
        self.chain.iter().skip(1).fold(0u128, |total, block| {
            total.saturating_add(Self::block_work(block.header.target))
        })
    }

    pub fn next_target(&self) -> u64 {
        Self::expected_target_for_chain(&self.params, &self.chain)
    }

    pub fn next_difficulty_bits(&self) -> u32 {
        Block::target_difficulty_bits(self.next_target())
    }

    /// Retorna se um bit de sinalização alcançou o limiar acordado na janela
    /// recente. A ativação de uma regra continua sendo definida em código por
    /// uma release futura; sinais sozinhos nunca mudam consenso.
    pub fn upgrade_signal_has_threshold(&self, signal_bit: u32) -> bool {
        if signal_bit == 0 || self.height() < self.params.upgrade_vote_window_blocks {
            return false;
        }
        let window = self.params.upgrade_vote_window_blocks as usize;
        let signaled = self
            .chain
            .iter()
            .rev()
            .take(window)
            .filter(|block| block.index > 0 && block.header.upgrade_signals & signal_bit != 0)
            .count();
        let required = window
            .saturating_mul(self.params.upgrade_vote_threshold_percent as usize)
            .saturating_add(99)
            / 100;
        signaled >= required
    }

    fn block_work(target: u64) -> u128 {
        let space = u128::from(u64::MAX) + 1;
        space / (u128::from(target) + 1)
    }

    fn expected_target_for_chain(params: &ChainParams, chain: &[Block]) -> u64 {
        let Some(tip) = chain.last() else {
            return params.initial_target;
        };
        if tip.index < params.retarget_interval_blocks.saturating_mul(2)
            || tip.index % params.retarget_interval_blocks != 0
        {
            return if tip.index == 0 {
                params.initial_target
            } else {
                tip.header.target
            };
        }

        let offset = params.retarget_interval_blocks as usize;
        let Some(start) = chain
            .len()
            .checked_sub(offset + 1)
            .and_then(|index| chain.get(index))
        else {
            return tip.header.target;
        };
        let actual_ms = tip.header.timestamp.saturating_sub(start.header.timestamp) as u128;
        let expected_ms = u128::from(params.target_block_time_seconds)
            .saturating_mul(1_000)
            .saturating_mul(u128::from(params.retarget_interval_blocks));
        if expected_ms == 0 {
            return tip.header.target;
        }
        let factor = u128::from(params.max_retarget_factor.max(1));
        let observed_ms = actual_ms.clamp(expected_ms / factor, expected_ms.saturating_mul(factor));
        let next = u128::from(tip.header.target).saturating_mul(observed_ms) / expected_ms;
        next.clamp(1, u128::from(u64::MAX)) as u64
    }

    /// Troca a cadeia somente quando a candidata é válida e tem mais trabalho.
    /// Empates usam o hash do topo como desempate determinístico, evitando que
    /// nós permaneçam divididos entre dois forks de mesmo trabalho.
    /// O mempool local é reaplicado contra o novo conjunto de UTXO.
    pub fn replace_if_heavier(&mut self, candidate: Vec<Block>) -> Result<bool> {
        let candidate_utxo = Self::validate_chain_and_build_utxo(&self.params, &candidate)?;
        let candidate_work = candidate.iter().skip(1).fold(0u128, |total, block| {
            total.saturating_add(Self::block_work(block.header.target))
        });
        let local_work = self.cumulative_work();
        let candidate_tip = candidate
            .last()
            .map(|block| block.hash.as_str())
            .unwrap_or("");
        if candidate_work < local_work
            || (candidate_work == local_work && candidate_tip >= self.last_block().hash.as_str())
        {
            return Ok(false);
        }

        let old_mempool = std::mem::take(&mut self.mempool);
        self.chain = candidate;
        self.utxo_set = candidate_utxo;
        for transaction in old_mempool {
            let _ = self.add_to_mempool(transaction);
        }
        Ok(true)
    }

    fn validate_next_block(&self, block: &Block) -> Result<HashMap<UtxoKey, UtxoEntry>> {
        let previous = self.last_block();
        if block.index != previous.index + 1 {
            return Err(WynError::Validation("altura de bloco inválida".into()));
        }
        if block.header.prev_hash != previous.hash {
            return Err(WynError::Validation("hash anterior inválido".into()));
        }
        if block.header.timestamp < previous.header.timestamp {
            return Err(WynError::Validation(
                "timestamp anterior ao bloco precedente".into(),
            ));
        }
        let mut utxo = self.utxo_set.clone();
        let expected_target = self.next_target();
        Self::validate_and_apply_non_genesis_block(
            &self.params,
            expected_target,
            block,
            &mut utxo,
        )?;
        Ok(utxo)
    }

    fn validate_chain_and_build_utxo(
        params: &ChainParams,
        chain: &[Block],
    ) -> Result<HashMap<UtxoKey, UtxoEntry>> {
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
            let expected_target = Self::expected_target_for_chain(params, &chain[..index]);
            Self::validate_and_apply_non_genesis_block(params, expected_target, block, &mut utxo)?;
        }
        Ok(utxo)
    }

    fn validate_and_apply_non_genesis_block(
        params: &ChainParams,
        expected_target: u64,
        block: &Block,
        utxo: &mut HashMap<UtxoKey, UtxoEntry>,
    ) -> Result<()> {
        if block.header.network_id != params.network_id {
            return Err(WynError::Validation(format!(
                "network_id inválido no bloco {}",
                block.index
            )));
        }
        if block.header.version != params.consensus_version {
            return Err(WynError::Validation(format!(
                "versão de consenso inválida no bloco {}",
                block.index
            )));
        }
        let maximum_timestamp = Utc::now()
            .timestamp_millis()
            .saturating_add((params.max_future_block_time_seconds as i64).saturating_mul(1_000));
        if block.header.timestamp > maximum_timestamp {
            return Err(WynError::Validation(format!(
                "timestamp do bloco {} está além do limite futuro",
                block.index
            )));
        }
        if block.header.target != expected_target {
            return Err(WynError::Validation(format!(
                "alvo PoW inválido no bloco {}",
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
        if serde_json::to_vec(block)?.len() > params.max_block_size_bytes {
            return Err(WynError::Validation(format!(
                "bloco {} excede o tamanho máximo",
                block.index
            )));
        }

        let mut working = utxo.clone();
        let mut fees = 0u64;
        for transaction in block.transactions.iter().skip(1) {
            let fee = Self::validate_regular_transaction(
                params,
                block.index,
                transaction,
                &working,
                &HashSet::new(),
            )?;
            Self::apply_regular_transaction(transaction, &mut working, block.index);
            fees = fees
                .checked_add(fee)
                .ok_or_else(|| WynError::Validation("overflow nas taxas".into()))?;
        }

        let coinbase = &block.transactions[0];
        Self::validate_coinbase(coinbase, block.index, params.block_reward, fees)?;
        Self::apply_coinbase(coinbase, &mut working, block.index);
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
            return Err(WynError::Validation(
                "estrutura da coinbase inválida".into(),
            ));
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
            return Err(WynError::Validation(
                "recompensa da coinbase inválida".into(),
            ));
        }
        Ok(())
    }

    fn validate_regular_transaction(
        params: &ChainParams,
        spending_height: u64,
        transaction: &Transaction,
        utxo: &HashMap<UtxoKey, UtxoEntry>,
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
            if derived_address != previous_output.output.recipient {
                return Err(WynError::Validation(
                    "a chave pública não pertence ao UTXO".into(),
                ));
            }
            if previous_output.is_coinbase
                && !Self::is_spendable_at(params, previous_output, spending_height)
            {
                return Err(WynError::Validation(
                    "coinbase ainda não atingiu maturidade".into(),
                ));
            }
            if !Wallet::verify(&public_pem, &signing_bytes, &input.signature) {
                return Err(WynError::Validation("assinatura inválida".into()));
            }
            input_total = input_total
                .checked_add(previous_output.output.amount)
                .ok_or_else(|| WynError::Validation("overflow nos inputs".into()))?;
        }

        let output_total = transaction.checked_total_output()?;
        input_total
            .checked_sub(output_total)
            .ok_or_else(|| WynError::Validation("saldo insuficiente".into()))
    }

    fn apply_regular_transaction(
        transaction: &Transaction,
        utxo: &mut HashMap<UtxoKey, UtxoEntry>,
        height: u64,
    ) {
        for input in &transaction.inputs {
            utxo.remove(&(input.tx_id.clone(), input.output_index));
        }
        for (output_index, output) in transaction.outputs.iter().enumerate() {
            utxo.insert(
                (transaction.id.clone(), output_index),
                UtxoEntry {
                    output: output.clone(),
                    created_height: height,
                    is_coinbase: false,
                },
            );
        }
    }

    fn apply_coinbase(
        transaction: &Transaction,
        utxo: &mut HashMap<UtxoKey, UtxoEntry>,
        height: u64,
    ) {
        for (output_index, output) in transaction.outputs.iter().enumerate() {
            utxo.insert(
                (transaction.id.clone(), output_index),
                UtxoEntry {
                    output: output.clone(),
                    created_height: height,
                    is_coinbase: true,
                },
            );
        }
    }

    fn is_spendable_at(params: &ChainParams, entry: &UtxoEntry, spending_height: u64) -> bool {
        !entry.is_coinbase
            || spending_height
                >= entry
                    .created_height
                    .saturating_add(params.coinbase_maturity_blocks)
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
            consensus_version: 4,
            initial_target: u64::MAX,
            block_reward: 5_000_000_000,
            target_block_time_seconds: 1,
            retarget_interval_blocks: 2,
            max_retarget_factor: 4,
            max_transactions_per_block: 100,
            max_block_size_bytes: 2 * 1024 * 1024,
            coinbase_maturity_blocks: 1,
            max_future_block_time_seconds: 30,
            upgrade_vote_window_blocks: 20,
            upgrade_vote_threshold_percent: 90,
            upgrade_activation_delay_blocks: 10,
        }
    }

    #[test]
    fn mines_and_validates_chain() {
        let wallet = Wallet::generate().unwrap();
        let mut blockchain = Blockchain::new(params());
        blockchain.mine_and_commit(&wallet.address).unwrap();
        blockchain.validate_full_chain().unwrap();
        assert_eq!(
            blockchain.balance_of(&wallet.address).unwrap(),
            5_000_000_000
        );
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
    fn accepts_blocks_without_a_hard_time_gate() {
        let wallet = Wallet::generate().unwrap();
        let mut blockchain = Blockchain::new(params());
        blockchain.mine_and_commit(&wallet.address).unwrap();

        let mut candidate = blockchain.build_candidate_block(&wallet.address).unwrap();
        candidate.mine().unwrap();
        blockchain.commit_block(candidate).unwrap();
    }

    #[test]
    fn only_replaces_chain_with_more_accumulated_work() {
        let wallet = Wallet::generate().unwrap();
        let mut local = Blockchain::new(params());
        local.mine_and_commit(&wallet.address).unwrap();

        let mut remote = Blockchain::new(params());
        remote.mine_and_commit(&wallet.address).unwrap();
        remote.mine_and_commit(&wallet.address).unwrap();

        assert!(local.replace_if_heavier(remote.chain.clone()).unwrap());
        assert_eq!(local.height(), 2);
        assert!(!local.replace_if_heavier(remote.chain).unwrap());
    }

    #[test]
    fn retarget_reduces_target_after_a_fast_window() {
        let wallet = Wallet::generate().unwrap();
        let mut params = params();
        params.initial_target = u64::MAX / 2;
        let mut blockchain = Blockchain::new(params.clone());
        let base = blockchain.last_block().header.timestamp;

        for offset in 1..=4 {
            let mut candidate = blockchain.build_candidate_block(&wallet.address).unwrap();
            candidate.header.timestamp = base + offset;
            candidate.mine().unwrap();
            blockchain.commit_block(candidate).unwrap();
        }

        assert!(blockchain.next_target() < params.initial_target);
    }

    #[test]
    fn coinbase_only_becomes_spendable_after_maturity() {
        let miner = Wallet::generate().unwrap();
        let recipient = Wallet::generate().unwrap();
        let mut params = params();
        params.coinbase_maturity_blocks = 3;
        let mut blockchain = Blockchain::new(params);
        blockchain.mine_and_commit(&miner.address).unwrap();

        assert!(blockchain.get_utxos_for(&miner.address).is_empty());
        blockchain.mine_and_commit(&miner.address).unwrap();
        assert!(blockchain.get_utxos_for(&miner.address).is_empty());
        blockchain.mine_and_commit(&miner.address).unwrap();

        let transaction = miner
            .build_transaction(
                &blockchain.get_utxos_for(&miner.address),
                &recipient.address,
                1_000_000_000,
                0,
            )
            .unwrap();
        blockchain.add_to_mempool(transaction).unwrap();
    }

    #[test]
    fn rejects_a_block_with_a_timestamp_too_far_in_the_future() {
        let wallet = Wallet::generate().unwrap();
        let mut blockchain = Blockchain::new(params());
        let mut candidate = blockchain.build_candidate_block(&wallet.address).unwrap();
        candidate.header.timestamp = Utc::now().timestamp_millis() + 31_000;
        candidate.mine().unwrap();

        assert!(blockchain.commit_block(candidate).is_err());
    }

    #[test]
    fn refuses_to_build_a_block_larger_than_the_consensus_limit() {
        let wallet = Wallet::generate().unwrap();
        let mut params = params();
        params.max_block_size_bytes = 1;
        let blockchain = Blockchain::new(params);

        assert!(blockchain.build_candidate_block(&wallet.address).is_err());
    }
}
