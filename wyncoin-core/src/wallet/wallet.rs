use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::rngs::OsRng;
use rsa::pkcs1v15::{SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::sha2::Sha256;
use rsa::signature::{RandomizedSigner, SignatureEncoding, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256 as RawSha256};
use zeroize::Zeroize;

use crate::blockchain::{Transaction, TxInput, TxOutput, Utxo};
use crate::{Result, WynError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub private_key_pem: String,
    pub public_key_pem: String,
    pub address: String,
}

impl Drop for Wallet {
    fn drop(&mut self) {
        self.private_key_pem.zeroize();
    }
}

impl Wallet {
    pub fn generate() -> Result<Self> {
        let mut rng = OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|error| WynError::Crypto(format!("falha ao gerar RSA: {error}")))?;
        let public_key = RsaPublicKey::from(&private_key);

        let private_key_pem = private_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|error| {
                WynError::Crypto(format!("falha ao codificar chave privada: {error}"))
            })?
            .to_string();
        let public_key_pem = public_key
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|error| {
                WynError::Crypto(format!("falha ao codificar chave pública: {error}"))
            })?;
        let address = Self::address_from_public_pem(&public_key_pem);

        Ok(Self {
            private_key_pem,
            public_key_pem,
            address,
        })
    }

    pub fn sign(&self, data: &[u8]) -> Result<String> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(&self.private_key_pem)
            .map_err(|error| WynError::Crypto(format!("chave privada inválida: {error}")))?;
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let mut rng = OsRng;
        let signature = signing_key.sign_with_rng(&mut rng, data);
        Ok(B64.encode(signature.to_bytes()))
    }

    pub fn verify(public_key_pem: &str, data: &[u8], signature_b64: &str) -> bool {
        let signature_bytes = match B64.decode(signature_b64) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        let public_key = match RsaPublicKey::from_public_key_pem(public_key_pem) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let signature = match rsa::pkcs1v15::Signature::try_from(signature_bytes.as_slice()) {
            Ok(signature) => signature,
            Err(_) => return false,
        };
        VerifyingKey::<Sha256>::new(public_key)
            .verify(data, &signature)
            .is_ok()
    }

    pub fn address_from_public_pem(public_key_pem: &str) -> String {
        let mut hasher = RawSha256::new();
        hasher.update(public_key_pem.as_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("WYN{}", &digest[..40])
    }

    pub fn public_key_b64(&self) -> String {
        B64.encode(self.public_key_pem.as_bytes())
    }

    pub fn public_key_pem_from_b64(value: &str) -> Option<String> {
        B64.decode(value)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let body = serde_json::to_vec_pretty(self)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(path)?;
        file.write_all(&body)?;
        file.sync_all()?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let body = fs::read(path)?;
        let wallet: Self = serde_json::from_slice(&body)?;
        let derived = Self::address_from_public_pem(&wallet.public_key_pem);
        if wallet.address != derived {
            return Err(WynError::Validation(
                "o endereço salvo não corresponde à chave pública".into(),
            ));
        }
        Ok(wallet)
    }

    pub fn build_transaction(
        &self,
        available_utxos: &[Utxo],
        recipient: &str,
        amount: u64,
        fee: u64,
    ) -> Result<Transaction> {
        if recipient.trim().is_empty() || recipient == self.address.as_str() {
            return Err(WynError::Validation(
                "destinatário inválido ou igual ao remetente".into(),
            ));
        }
        if amount == 0 {
            return Err(WynError::Validation(
                "o valor deve ser maior que zero".into(),
            ));
        }

        let required = amount
            .checked_add(fee)
            .ok_or_else(|| WynError::Validation("overflow no valor da transação".into()))?;
        let mut selected = Vec::new();
        let mut selected_total = 0u64;

        for utxo in available_utxos {
            if utxo.output.recipient != self.address {
                continue;
            }
            selected.push(utxo.clone());
            selected_total = selected_total
                .checked_add(utxo.output.amount)
                .ok_or_else(|| WynError::Validation("overflow na seleção de UTXOs".into()))?;
            if selected_total >= required {
                break;
            }
        }

        if selected_total < required {
            return Err(WynError::Validation("saldo confirmado insuficiente".into()));
        }

        let public_key = self.public_key_b64();
        let inputs = selected
            .iter()
            .map(|utxo| TxInput {
                tx_id: utxo.tx_id.clone(),
                output_index: utxo.output_index,
                signature: String::new(),
                public_key: public_key.clone(),
            })
            .collect();

        let mut outputs = vec![TxOutput {
            recipient: recipient.to_string(),
            amount,
        }];
        let change = selected_total - required;
        if change > 0 {
            outputs.push(TxOutput {
                recipient: self.address.clone(),
                amount: change,
            });
        }

        let mut transaction = Transaction::new(inputs, outputs)?;
        let signature = self.sign(&transaction.signing_bytes()?)?;
        for input in &mut transaction.inputs {
            input.signature = signature.clone();
        }
        transaction.refresh_id()?;
        Ok(transaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let wallet = Wallet::generate().unwrap();
        let message = b"wyncoin";
        let signature = wallet.sign(message).unwrap();
        assert!(Wallet::verify(&wallet.public_key_pem, message, &signature));
    }
}
