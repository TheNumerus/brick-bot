use thiserror::Error;
#[derive(Error, Debug)]
pub enum BotError {
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    ImageError(#[from] image::error::ImageError),
    #[error("Internal Error")]
    InternalError,
    #[error("Api Error: `{0}`")]
    ApiError(String),
}
