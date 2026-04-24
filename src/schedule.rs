use anyhow::{bail, Context, Result};
use chrono::NaiveTime;

pub fn parse_time(time: &str) -> Result<NaiveTime> {
    if time.len() != 5 || time.as_bytes().get(2) != Some(&b':') {
        bail!("time must be in 24-hour HH:MM format");
    }

    NaiveTime::parse_from_str(time, "%H:%M").context("time must be in 24-hour HH:MM format")
}

pub fn parse_repeat_interval(every: &str) -> Result<std::time::Duration> {
    let duration = humantime::parse_duration(every)
        .context("repeat interval must look like 5m, 30m, or 1h")?;
    if duration.is_zero() {
        bail!("repeat interval must be greater than zero");
    }
    Ok(duration)
}

pub fn parse_window_duration(value: &str) -> Result<std::time::Duration> {
    let duration =
        humantime::parse_duration(value).context("--for must look like 30m, 2h, or 3h10m")?;
    if duration.is_zero() {
        bail!("--for must be greater than zero");
    }
    if duration >= std::time::Duration::from_secs(24 * 60 * 60) {
        bail!("--for must be shorter than 24h to avoid overlapping daily windows");
    }
    Ok(duration)
}
