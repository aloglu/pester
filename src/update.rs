use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::{term, version};

const REPO: &str = "aloglu/pester";
const BIN_NAME: &str = "pester";

pub fn run(paths: &crate::paths::Paths) -> Result<()> {
    platform::run(paths)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::*;

    pub fn run(_paths: &crate::paths::Paths) -> Result<()> {
        let install = InstallLayout::detect()?;
        let status = version::check_for_update()?;

        if !status.is_update_available() {
            term::ok(format!(
                "pester {} is already up to date.",
                status.current_version
            ));
            return Ok(());
        }

        term::heading("pester update");
        term::detail(format!("Current version: {}", status.current_version));
        term::detail(format!("Latest version: {}", status.latest_version));

        let artifact = artifact_name()?;
        let base_url = format!("https://github.com/{REPO}/releases/latest/download");
        let temp_dir = TempDir::new("pester-update")?;
        let artifact_path = temp_dir.path().join(&artifact);
        let checksums_path = temp_dir.path().join("checksums.txt");
        let checksum_line_path = temp_dir.path().join("checksum.txt");

        term::detail(format!("Downloading {artifact}"));
        download(&format!("{base_url}/{artifact}"), &artifact_path)?;
        download(&format!("{base_url}/checksums.txt"), &checksums_path)?;

        let checksum_line = checksum_entry(&artifact, &checksums_path)?;
        fs::write(&checksum_line_path, format!("{checksum_line}\n"))
            .with_context(|| format!("failed to write {}", checksum_line_path.display()))?;
        verify_checksum(temp_dir.path(), &checksum_line_path)?;

        extract_archive(&artifact_path, temp_dir.path())?;
        install_binary(&temp_dir.path().join(BIN_NAME), &install.binary_path)?;
        if let Some(app_path) = install.app_path.as_ref() {
            install_app_bundle(&temp_dir.path().join("pester.app"), app_path)?;
        }

        restart_service(&install.binary_path)?;
        term::ok(format!(
            "Updated pester from {} to {}.",
            status.current_version, status.latest_version
        ));
        Ok(())
    }

    #[derive(Debug, Clone)]
    struct InstallLayout {
        binary_path: PathBuf,
        app_path: Option<PathBuf>,
    }

    impl InstallLayout {
        fn detect() -> Result<Self> {
            let home = home_dir()?;
            let managed_binary = home.join(".local/bin/pester");
            let current =
                std::env::current_exe().context("could not determine current executable")?;

            #[cfg(target_os = "linux")]
            if current != managed_binary {
                bail!(
                    "pester update only supports the managed install at {}. Current executable: {}",
                    managed_binary.display(),
                    current.display()
                );
            }

            #[cfg(target_os = "macos")]
            {
                let managed_app_binary = home.join("Applications/pester.app/Contents/MacOS/pester");
                if current != managed_binary && current != managed_app_binary {
                    bail!(
                        "pester update only supports the managed install at {}. Current executable: {}",
                        managed_binary.display(),
                        current.display()
                    );
                }
                return Ok(Self {
                    binary_path: managed_binary,
                    app_path: Some(home.join("Applications/pester.app")),
                });
            }

            #[cfg(target_os = "linux")]
            Ok(Self {
                binary_path: managed_binary,
                app_path: None,
            })
        }
    }

    fn artifact_name() -> Result<String> {
        artifact_name_for(std::env::consts::OS, std::env::consts::ARCH)
    }

    fn artifact_name_for(os: &str, arch: &str) -> Result<String> {
        let target_arch = match arch {
            "x86_64" => {
                if os == "macos" {
                    bail!("Intel macOS is not supported");
                }
                "x86_64"
            }
            "aarch64" | "arm64" => "aarch64",
            _ => bail!("unsupported architecture: {arch}"),
        };

        match os {
            "linux" => Ok(format!("pester-linux-{target_arch}.tar.gz")),
            "macos" => Ok(format!("pester-macos-{target_arch}.tar.gz")),
            _ => bail!("unsupported OS: {os}"),
        }
    }

    fn home_dir() -> Result<PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("could not determine home directory")
    }

    fn download(url: &str, destination: &Path) -> Result<()> {
        let status = Command::new("curl")
            .args(["-fsSL", url, "-o"])
            .arg(destination)
            .status()
            .with_context(|| format!("failed to run curl for {url}"))?;
        if !status.success() {
            bail!("curl failed with status {status}");
        }
        Ok(())
    }

    fn checksum_entry(artifact: &str, checksums_path: &Path) -> Result<String> {
        let content = fs::read_to_string(checksums_path)
            .with_context(|| format!("failed to read {}", checksums_path.display()))?;
        content
            .lines()
            .find(|line| line.ends_with(&format!("  {artifact}")))
            .map(ToOwned::to_owned)
            .with_context(|| format!("checksum entry not found for {artifact}"))
    }

    fn verify_checksum(temp_dir: &Path, checksum_path: &Path) -> Result<()> {
        let result = if command_exists("sha256sum") {
            Command::new("sha256sum")
                .args([
                    "-c",
                    checksum_path
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or(""),
                ])
                .current_dir(temp_dir)
                .status()
                .context("failed to run sha256sum")?
        } else if command_exists("shasum") {
            Command::new("shasum")
                .args([
                    "-a",
                    "256",
                    "-c",
                    checksum_path
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or(""),
                ])
                .current_dir(temp_dir)
                .status()
                .context("failed to run shasum")?
        } else {
            bail!("could not verify checksum: sha256sum or shasum is required");
        };

        if !result.success() {
            bail!("checksum verification failed");
        }
        Ok(())
    }

    fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
        let status = Command::new("tar")
            .args(["-xzf"])
            .arg(archive_path)
            .args(["-C"])
            .arg(destination)
            .status()
            .context("failed to run tar")?;
        if !status.success() {
            bail!("tar failed with status {status}");
        }
        Ok(())
    }

    fn install_binary(source: &Path, destination: &Path) -> Result<()> {
        let parent = destination
            .parent()
            .with_context(|| format!("{} has no parent directory", destination.display()))?;
        fs::create_dir_all(parent)?;
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        make_executable(destination)?;
        Ok(())
    }

    fn install_app_bundle(source: &Path, destination: &Path) -> Result<()> {
        if !source.exists() {
            bail!("downloaded release did not contain pester.app");
        }
        if destination.exists() {
            fs::remove_dir_all(destination)
                .with_context(|| format!("failed to remove {}", destination.display()))?;
        }
        copy_dir_all(source, destination)?;
        make_executable(&destination.join("Contents/MacOS/pester"))?;
        Ok(())
    }

    fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
        fs::create_dir_all(destination)?;
        for entry in
            fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
        {
            let entry = entry?;
            let entry_type = entry.file_type()?;
            let from = entry.path();
            let to = destination.join(entry.file_name());
            if entry_type.is_dir() {
                copy_dir_all(&from, &to)?;
            } else {
                fs::copy(&from, &to).with_context(|| {
                    format!("failed to copy {} to {}", from.display(), to.display())
                })?;
            }
        }
        Ok(())
    }

    fn restart_service(binary_path: &Path) -> Result<()> {
        let output = Command::new(binary_path)
            .args(["system", "install"])
            .output()
            .with_context(|| format!("failed to run {} system install", binary_path.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "background service installation failed.\n{}\n{}",
                stdout.trim(),
                stderr.trim()
            );
        }
        Ok(())
    }

    fn command_exists(program: &str) -> bool {
        Command::new("sh")
            .args(["-c", &format!("command -v {program} >/dev/null 2>&1")])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn make_executable(path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)
                .with_context(|| format!("failed to set permissions on {}", path.display()))?;
        }
        Ok(())
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock is before UNIX_EPOCH")?
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::artifact_name_for;

        #[test]
        fn selects_linux_artifacts() {
            assert_eq!(
                artifact_name_for("linux", "x86_64").unwrap(),
                "pester-linux-x86_64.tar.gz"
            );
            assert_eq!(
                artifact_name_for("linux", "aarch64").unwrap(),
                "pester-linux-aarch64.tar.gz"
            );
        }

        #[test]
        fn selects_macos_artifacts() {
            assert_eq!(
                artifact_name_for("macos", "aarch64").unwrap(),
                "pester-macos-aarch64.tar.gz"
            );
        }

        #[test]
        fn rejects_unsupported_targets() {
            assert!(artifact_name_for("macos", "x86_64").is_err());
            assert!(artifact_name_for("linux", "sparc").is_err());
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use anyhow::{bail, Result};

    pub fn run(_paths: &crate::paths::Paths) -> Result<()> {
        bail!("pester update is only supported on Linux and macOS")
    }
}
