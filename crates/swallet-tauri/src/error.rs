use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl From<String> for CommandError {
    fn from(s: String) -> Self {
        CommandError { message: s }
    }
}

impl From<&str> for CommandError {
    fn from(s: &str) -> Self {
        CommandError { message: s.to_string() }
    }
}

impl From<swallet_core::error::StorageError> for CommandError {
    fn from(e: swallet_core::error::StorageError) -> Self {
        CommandError { message: e.to_string() }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
