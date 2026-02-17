//! Error types for rustloclib

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during LOC counting
#[derive(Error, Debug)]
pub enum RustlocError {
    /// Failed to read a file
    #[error("failed to read file '{path}': {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to parse Cargo.toml or workspace metadata
    #[error("failed to parse cargo metadata: {0}")]
    CargoMetadata(String),

    /// Invalid glob pattern
    #[error("invalid glob pattern '{pattern}': {message}")]
    InvalidGlob { pattern: String, message: String },

    /// Path does not exist
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),

    /// No Cargo.toml found at or above path
    #[error("no Cargo.toml found at or above: {0}")]
    CargoTomlNotFound(PathBuf),

    /// Not a Rust file
    #[error("not a Rust file: {0}")]
    NotRustFile(PathBuf),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Git operation error
    #[error("git error: {0}")]
    GitError(String),
}
