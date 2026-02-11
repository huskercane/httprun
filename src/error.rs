use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("Environment error: {0}")]
    Environment(String),

    #[error("Variable not found: {{{{{0}}}}}")]
    #[allow(dead_code)]
    VariableNotFound(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JavaScript error: {0}")]
    JavaScript(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
