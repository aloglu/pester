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

    pub fn delete_installed_binaries(&self) -> Result<Vec<PathBuf>> {
        let current = std::env::current_exe()?;
        delete_installed_binaries(&current)
    }
}

#[cfg(not(target_os = "windows"))]
fn delete_installed_binaries(current: &Path) -> Result<Vec<PathBuf>> {
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
        return Ok(Vec::new());
    };
    let installed = home.join(".local/bin/pester");
    if current == installed && current.exists() {
        fs::remove_file(current)
            .with_context(|| format!("failed to remove {}", current.display()))?;
        return Ok(vec![current.to_path_buf()]);
    }
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
fn delete_installed_binaries(current: &Path) -> Result<Vec<PathBuf>> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Ok(Vec::new());
    };
    let installed = PathBuf::from(local_app_data)
        .join("Programs")
        .join("pester")
        .join("pester.exe");
    if !windows_path_eq(current, &installed) || !current.exists() {
        return Ok(Vec::new());
    }
    let daemon = installed.with_file_name("pesterd.exe");
    let mut targets = vec![current.to_path_buf()];
    if daemon.exists() {
        targets.push(daemon);
    }

    schedule_windows_self_delete(&targets)
        .with_context(|| format!("failed to schedule removal of {}", current.display()))?;
    Ok(targets)
}

#[cfg(target_os = "windows")]
fn schedule_windows_self_delete(targets: &[PathBuf]) -> Result<()> {
    use std::os::windows::process::CommandExt;

    let quoted_targets = targets
        .iter()
        .map(|path| format!("'{}'", path.display().to_string().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let script = format!(
        "$Paths = @({quoted_targets}); \
         Start-Sleep -Milliseconds 500; \
         for ($i = 0; $i -lt 20; $i++) {{ \
           $Failed = $false; \
           foreach ($Path in $Paths) {{ \
             if (Test-Path -LiteralPath $Path) {{ \
               try {{ Remove-Item -LiteralPath $Path -Force -ErrorAction Stop }} \
               catch {{ $Failed = $true }} \
             }} \
           }} \
           if (-not $Failed) {{ exit 0 }} \
           Start-Sleep -Milliseconds 500; \
         }}; \
         exit 1"
    );
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("failed to start Windows self-delete helper")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_path_eq(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::NaiveDate;

    use super::Store;
    use crate::models::{Config, Reminder, State};
    use crate::paths::Paths;

    fn temp_store(name: &str) -> (std::path::PathBuf, Store) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pester-{name}-{unique}"));
        let config_dir = root.join("config");
        let state_dir = root.join("state");
        let paths = Paths {
            config_file: config_dir.join("config.toml"),
            state_file: state_dir.join("state.json"),
            config_dir,
            state_dir,
        };
        (root, Store { paths })
    }

    #[test]
    fn saves_and_loads_config_and_state() {
        let (root, store) = temp_store("roundtrip");
        let config = Config {
            reminders: vec![Reminder {
                id: "winddown".to_string(),
                title: "Wind down".to_string(),
                message: "No exciting stuff now.".to_string(),
                time: "22:00".to_string(),
                repeat_every: "5m".to_string(),
                until: None,
                active_for: None,
                max_notifications: None,
                done_phrase: None,
                enabled: true,
            }],
            confirmation: Default::default(),
        };
        let date = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();
        let mut state = State::default();
        state.mark_done(date, "winddown");

        store.save_config(&config).unwrap();
        store.save_state(&state).unwrap();

        let loaded_config = store.load_config().unwrap();
        let loaded_state = store.load_state().unwrap();
        assert_eq!(loaded_config.reminders[0].id, "winddown");
        assert!(loaded_state.get(date, "winddown").unwrap().done);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn delete_data_removes_config_and_state_directories() {
        let (root, store) = temp_store("delete-data");
        std::fs::create_dir_all(&store.paths.config_dir).unwrap();
        std::fs::create_dir_all(&store.paths.state_dir).unwrap();

        store.delete_data().unwrap();

        assert!(!store.paths.config_dir.exists());
        assert!(!store.paths.state_dir.exists());

        std::fs::remove_dir_all(root).ok();
    }
}
