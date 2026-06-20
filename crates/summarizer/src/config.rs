use std::path::PathBuf;

/// Minimal Config surface used by the summarizer crate.
#[derive(Debug, Clone)]
pub struct Config {
    pub workspace_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace_dir: PathBuf::from(".")
        }
    }
}

