use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{Result, Wallet, WynError};

const KEYSTORE_VERSION: u32 = 1;
const KDF_MEMORY_KIB: u32 = 19_456;
const KDF_ITERATIONS: u32 = 2;
const KDF_PARALLELISM: u32 = 1;
const MIN_PASSWORD_LENGTH: usize = 8;
const AAD_PREFIX: &[u8] = b"wyncoin-wallet-keystore-v1:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletMetadata {
    pub version: u32,
    pub address: String,
    pub public_key_pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedWalletFile {
    pub version: u32,
    pub address: String,
    pub public_key_pem: String,
    pub kdf: String,
    pub salt_b64: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Serialize, Deserialize)]
struct SecretPayload {
    private_key_pem: String,
}

impl EncryptedWalletFile {
    pub fn create(password: &str) -> Result<(Self, Wallet)> {
        let wallet = Wallet::generate()?;
        let file = Self::from_wallet(&wallet, password)?;
        Ok((file, wallet))
    }

    pub fn from_wallet(wallet: &Wallet, password: &str) -> Result<Self> {
        let password = validated_password(password)?;
        let payload = SecretPayload {
            private_key_pem: wallet.private_key_pem.clone(),
        };
        let plaintext = Zeroizing::new(serde_json::to_vec(&payload)?);
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 24];
        let mut rng = OsRng;
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut nonce);
        let key = derive_key(&password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| WynError::Crypto("não foi possível inicializar a cifra".into()))?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad(&wallet.address),
                },
            )
            .map_err(|_| WynError::Crypto("não foi possível criptografar a carteira".into()))?;

        Ok(Self {
            version: KEYSTORE_VERSION,
            address: wallet.address.clone(),
            public_key_pem: wallet.public_key_pem.clone(),
            kdf: "argon2id".into(),
            salt_b64: B64.encode(salt),
            nonce_b64: B64.encode(nonce),
            ciphertext_b64: B64.encode(ciphertext),
        })
    }

    pub fn metadata(&self) -> Result<WalletMetadata> {
        self.validate_public_metadata()?;
        Ok(WalletMetadata {
            version: self.version,
            address: self.address.clone(),
            public_key_pem: self.public_key_pem.clone(),
        })
    }

    pub fn unlock(&self, password: &str) -> Result<Wallet> {
        self.validate_public_metadata()?;
        let password = validated_password(password)?;
        let salt = decode_exact::<16>(&self.salt_b64, "salt da carteira")?;
        let nonce = decode_exact::<24>(&self.nonce_b64, "nonce da carteira")?;
        let ciphertext = B64
            .decode(&self.ciphertext_b64)
            .map_err(|_| WynError::Crypto("ciphertext da carteira inválido".into()))?;
        let key = derive_key(&password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| WynError::Crypto("não foi possível inicializar a cifra".into()))?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad(&self.address),
                    },
                )
                .map_err(|_| WynError::Crypto("senha incorreta ou carteira corrompida".into()))?,
        );
        let mut payload: SecretPayload = serde_json::from_slice(&plaintext)
            .map_err(|_| WynError::Crypto("conteúdo protegido da carteira inválido".into()))?;
        let wallet = Wallet {
            private_key_pem: payload.private_key_pem.clone(),
            public_key_pem: self.public_key_pem.clone(),
            address: self.address.clone(),
        };
        payload.private_key_pem.zeroize();
        let derived = Wallet::address_from_public_pem(&wallet.public_key_pem);
        if derived != wallet.address {
            return Err(WynError::Crypto(
                "metadados da carteira não conferem".into(),
            ));
        }
        // A assinatura prova que a chave privada descriptografada corresponde
        // à chave pública anunciada, sem revelar material secreto à interface.
        let signature = wallet.sign(b"wyncoin-keystore-verification")?;
        if !Wallet::verify(
            &wallet.public_key_pem,
            b"wyncoin-keystore-verification",
            &signature,
        ) {
            return Err(WynError::Crypto(
                "a chave privada não corresponde ao endereço".into(),
            ));
        }
        Ok(wallet)
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_vec_pretty(self)?;
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&body)?;
        file.sync_all()?;
        Ok(())
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let body = fs::read(path)?;
        let file: Self = serde_json::from_slice(&body)?;
        file.metadata()?;
        Ok(file)
    }

    fn validate_public_metadata(&self) -> Result<()> {
        if self.version != KEYSTORE_VERSION {
            return Err(WynError::Validation(
                "versão de carteira protegida não suportada".into(),
            ));
        }
        if self.kdf != "argon2id" {
            return Err(WynError::Validation(
                "algoritmo de derivação de chave não suportado".into(),
            ));
        }
        if self.address != Wallet::address_from_public_pem(&self.public_key_pem) {
            return Err(WynError::Validation(
                "o endereço protegido não corresponde à chave pública".into(),
            ));
        }
        Ok(())
    }
}

fn validated_password(password: &str) -> Result<Zeroizing<String>> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(WynError::Validation(format!(
            "a senha da carteira deve ter ao menos {MIN_PASSWORD_LENGTH} caracteres"
        )));
    }
    Ok(Zeroizing::new(password.to_owned()))
}

fn derive_key(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(KDF_MEMORY_KIB, KDF_ITERATIONS, KDF_PARALLELISM, Some(32))
        .map_err(|error| WynError::Crypto(format!("parâmetros Argon2 inválidos: {error}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|error| WynError::Crypto(format!("falha ao derivar chave: {error}")))?;
    Ok(key)
}

fn decode_exact<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    let bytes = B64
        .decode(value)
        .map_err(|_| WynError::Crypto(format!("{label} inválido")))?;
    bytes
        .try_into()
        .map_err(|_| WynError::Crypto(format!("{label} possui tamanho inválido")))
}

fn aad(address: &str) -> Vec<u8> {
    [AAD_PREFIX, address.as_bytes()].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_unlocks_a_wallet() {
        let (file, original) = EncryptedWalletFile::create("uma senha segura").unwrap();
        let unlocked = file.unlock("uma senha segura").unwrap();
        assert_eq!(unlocked.address, original.address);
        assert!(file.unlock("senha errada demais").is_err());
    }
}
