use thiserror::Error;

#[derive(Debug, Error)]
pub enum WynError {
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("erro de banco de dados: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON inválido: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML inválido: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("configuração inválida: {0}")]
    Config(String),

    #[error("dados inválidos: {0}")]
    Validation(String),

    #[error("erro criptográfico: {0}")]
    Crypto(String),

    #[error("erro de protocolo: {0}")]
    Protocol(String),
}
