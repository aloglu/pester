use std::cmp::Ordering;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const RELEASES_API_URL: &str = "https://api.github.com/repos/aloglu/pester/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: String,
}

impl UpdateStatus {
    pub fn is_update_available(&self) -> bool {
        compare_versions(&self.current_version, &self.latest_version)
            .map(|ordering| ordering.is_lt())
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
}

pub fn latest_release() -> Result<ReleaseInfo> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: pester",
            RELEASES_API_URL,
        ])
        .output()
        .context("failed to run curl for release metadata")?;
    if !output.status.success() {
        bail!("curl failed with status {}", output.status);
    }

    let release: GithubRelease =
        serde_json::from_slice(&output.stdout).context("failed to parse GitHub release JSON")?;
    let version = normalize_tag(&release.tag_name)?;
    Ok(ReleaseInfo { version })
}

pub fn check_for_update() -> Result<UpdateStatus> {
    let latest = latest_release()?;
    Ok(UpdateStatus {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: latest.version,
    })
}

pub fn compare_versions(left: &str, right: &str) -> Result<Ordering> {
    let left = VersionNumber::parse(left)?;
    let right = VersionNumber::parse(right)?;
    Ok(left.cmp(&right))
}

fn normalize_tag(tag: &str) -> Result<String> {
    let normalized = tag.trim().trim_start_matches('v');
    VersionNumber::parse(normalized)?;
    Ok(normalized.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct VersionNumber {
    major: u64,
    minor: u64,
    patch: u64,
}

impl VersionNumber {
    fn parse(input: &str) -> Result<Self> {
        let mut parts = input.split('.');
        let major = parts
            .next()
            .context("missing major version")?
            .parse()
            .with_context(|| format!("invalid major version in {input}"))?;
        let minor = parts
            .next()
            .context("missing minor version")?
            .parse()
            .with_context(|| format!("invalid minor version in {input}"))?;
        let patch = parts
            .next()
            .context("missing patch version")?
            .parse()
            .with_context(|| format!("invalid patch version in {input}"))?;
        if parts.next().is_some() {
            bail!("unsupported version format: {input}");
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{compare_versions, normalize_tag, UpdateStatus, VersionNumber};

    #[test]
    fn parses_simple_semver() {
        let version = VersionNumber::parse("1.2.3").unwrap();

        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
    }

    #[test]
    fn rejects_non_three_part_versions() {
        assert!(VersionNumber::parse("1.2").is_err());
        assert!(VersionNumber::parse("1.2.3.4").is_err());
    }

    #[test]
    fn normalizes_v_prefixed_tags() {
        assert_eq!(normalize_tag("v0.1.8").unwrap(), "0.1.8");
        assert_eq!(normalize_tag("0.1.8").unwrap(), "0.1.8");
    }

    #[test]
    fn compares_versions_numerically() {
        assert_eq!(compare_versions("0.1.8", "0.1.9").unwrap(), Ordering::Less);
        assert_eq!(
            compare_versions("0.2.0", "0.1.9").unwrap(),
            Ordering::Greater
        );
        assert_eq!(compare_versions("1.0.0", "1.0.0").unwrap(), Ordering::Equal);
    }

    #[test]
    fn reports_update_availability() {
        let status = UpdateStatus {
            current_version: "0.1.8".to_string(),
            latest_version: "0.1.9".to_string(),
        };
        let current = UpdateStatus {
            current_version: "0.1.8".to_string(),
            latest_version: "0.1.8".to_string(),
        };

        assert!(status.is_update_available());
        assert!(!current.is_update_available());
    }
}
