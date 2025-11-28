use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Steam not running or init failed")]
    SteamInitFailed,
    #[error("Steam networking error: {0}")]
    SteamNetworking(String),
    #[error("Lobby error: {0}")]
    LobbyError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Lock error")]
    LockError,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

pub type AppResult<T> = Result<T, AppError>;