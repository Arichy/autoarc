use thiserror::Error;

#[derive(Debug, Error)]
pub enum AutoarcError {
    #[error("No correct password provided.")]
    NoCorrectPassword,

    #[error("Unknown Error: {0}")]
    Other(String),
}
