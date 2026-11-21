use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("não foi possível conectar ao Docker: {0}")]
    Connect(#[source] bollard::errors::Error),

    #[error("sem permissão para acessar o socket do Docker (adicione seu usuário ao grupo `docker` ou use DOCKER_HOST)")]
    PermissionDenied,

    #[error("erro da API do Docker: {0}")]
    Api(#[from] bollard::errors::Error),

    #[error("erro de serialização: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

impl CoreError {
    /// Classifica erros de conexão, detectando falta de permissão no socket.
    pub(crate) fn from_connect(err: bollard::errors::Error) -> Self {
        let msg = err.to_string().to_lowercase();
        if msg.contains("permission denied") {
            CoreError::PermissionDenied
        } else {
            CoreError::Connect(err)
        }
    }
}
