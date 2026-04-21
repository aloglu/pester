use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::models::{Config, State};
use crate::paths::Paths;

#[derive(Debug, Clone)]
pub struct Store {
    pub paths: Paths,
}

impl Store {
    pub fn new() -> Result<Self> {
        Ok(Self {
            paths: Paths::new()?,
        })
    }

    pub fn load_config(&self) -> Result<Config> {
        if !self.paths.config_file.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(&self.paths.config_file)
            .with_context(|| format!("failed to read {}", self.paths.config_file.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.paths.config_file.display()))
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        fs::create_dir_all(&self.paths.config_dir)?;
        let content = toml::to_string_pretty(config)?;
        write_atomic(&self.paths.config_file, content.as_bytes())
    }

    pub fn load_state(&self) -> Result<State> {
        if !self.paths.state_file.exists() {
            return Ok(State::default());
        }

        let content = fs::read_to_string(&self.paths.state_file)
            .with_context(|| format!("failed to read {}", self.paths.state_file.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.paths.state_file.display()))
    }

    pub fn save_state(&self, state: &State) -> Result<()> {
        fs::create_dir_all(&self.paths.state_dir)?;
        let content = serde_json::to_vec_pretty(state)?;
        write_atomic(&self.paths.state_file, &content)
    }

    pub fn delete_data(&self) -> Result<()> {
        if self.paths.config_dir.exists() {
            fs::remove_dir_all(&self.paths.config_dir)
                .with_context(|| format!("failed to remove {}", self.paths.config_dir.display()))?;
        }
        if self.paths.state_dir.exists() && self.paths.state_dir != self.paths.config_dir {
            fs::remove_dir_all(&self.paths.state_dir)
                .with_context(|| format!("failed to remove {}", self.paths.state_dir.display()))?;
        }
        Ok(())
    }

    pub fn delete_installed_binary(&self) -> Result<Option<PathBuf>> {
        let current = std::env::current_exe()?;
        delete_installed_binary(&current)
    }
}

#[cfg(not(target_os = "windows"))]
fn delete_installed_binary(current: &Path) -> Result<Option<PathBuf>> {
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
        return Ok(None);
    };
    let installed = home.join(".local/bin/pester");
    if current == installed && current.exists() {
        fs::remove_file(current)
            .with_context(|| format!("failed to remove {}", current.display()))?;
        return Ok(Some(current.to_path_buf()));
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
fn delete_installed_binary(current: &Path) -> Result<Option<PathBuf>> {
    use std::os::windows::process::CommandExt;

    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Ok(None);
    };
    let installed = PathBuf::from(local_app_data)
        .join("Programs")
        .join("Pester")
        .join("pester.exe");
    if current != installed || !current.exists() {
        return Ok(None);
    }

    let script = format!(
        "Start-Sleep -Milliseconds 500; \
         for ($i = 0; $i -lt 20; $i++) {{ \
           try {{ Remove-Item -LiteralPath '{}' -Force -ErrorAction Stop; exit 0 }} \
           catch {{ Start-Sleep -Milliseconds 500 }} \
         }}",
        current.display().to_string().replace('\'', "''")
    );
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| format!("failed to schedule removal of {}", current.display()))?;
    Ok(Some(current.to_path_buf()))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)?;

    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)
            .with_context(|| format!("failed to create {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}
