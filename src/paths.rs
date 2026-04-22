use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_file: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let project = ProjectDirs::from("", "aloglu", "pester")
            .context("could not determine platform config directories")?;

        let config_dir = project.config_dir().to_path_buf();
        let state_dir = project
            .state_dir()
            .unwrap_or_else(|| project.data_local_dir())
            .to_path_buf();

        Ok(Self {
            config_file: config_dir.join("config.toml"),
            state_file: state_dir.join("state.json"),
            config_dir,
            state_dir,
        })
    }
}
