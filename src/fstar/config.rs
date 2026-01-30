//! F* configuration handling.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] serde_json::Error),
}

/// F* configuration - all fields optional with sensible defaults
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FStarConfig {
    /// Include directories (--include paths)
    #[serde(default)]
    pub include_dirs: Vec<String>,

    /// Options to pass to fstar.exe
    #[serde(default)]
    pub options: Vec<String>,

    /// Path to fstar.exe (defaults to "fstar.exe" in PATH)
    #[serde(default)]
    pub fstar_exe: Option<String>,

    /// Working directory for fstar.exe
    #[serde(default)]
    pub cwd: Option<String>,
}

impl FStarConfig {
    /// Get the F* executable path (with default)
    pub fn fstar_exe(&self) -> &str {
        self.fstar_exe.as_deref().unwrap_or("fstar.exe")
    }

    /// Get the working directory (with default)
    pub fn cwd_or(&self, default: &Path) -> PathBuf {
        self.cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| default.to_path_buf())
    }

    /// Build command-line arguments for F* IDE mode
    pub fn build_args(&self, file_path: &str, lax: bool) -> Vec<String> {
        let mut args = vec!["--ide".to_string(), file_path.to_string()];

        if lax {
            args.push("--admit_smt_queries".to_string());
            args.push("true".to_string());
        }

        // Add custom options
        args.extend(self.options.clone());

        // Add include directories
        for dir in &self.include_dirs {
            args.push("--include".to_string());
            args.push(dir.clone());
        }

        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FStarConfig::default();
        assert_eq!(config.fstar_exe(), "fstar.exe");
        assert!(config.include_dirs.is_empty());
        assert!(config.options.is_empty());
    }

    #[test]
    fn test_build_args() {
        let config = FStarConfig {
            include_dirs: vec!["/path/to/lib".to_string()],
            options: vec!["--cache_dir".to_string(), ".cache".to_string()],
            fstar_exe: Some("fstar".to_string()),
            cwd: Some("/project".to_string()),
        };

        let args = config.build_args("/path/to/Test.fst", false);
        assert_eq!(args[0], "--ide");
        assert_eq!(args[1], "/path/to/Test.fst");
        assert!(args.contains(&"--include".to_string()));
        assert!(args.contains(&"/path/to/lib".to_string()));
        assert!(args.contains(&"--cache_dir".to_string()));
    }

    #[test]
    fn test_build_args_lax() {
        let config = FStarConfig::default();
        let args = config.build_args("Test.fst", true);
        assert!(args.contains(&"--admit_smt_queries".to_string()));
        assert!(args.contains(&"true".to_string()));
    }
}
