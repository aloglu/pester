use anyhow::Result;

use crate::paths::Paths;

pub fn install(paths: &Paths) -> Result<()> {
    platform::install(paths)
}

pub fn uninstall(paths: &Paths) -> Result<()> {
    platform::uninstall(paths)
}

pub fn diagnostics(paths: &Paths) -> Vec<String> {
    platform::diagnostics(paths)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use anyhow::{Context, Result};

    use crate::{paths::Paths, term};

    pub fn install(_paths: &Paths) -> Result<()> {
        let exe = std::env::current_exe()?;
        let service_dir = dirs_home()?.join(".config/systemd/user");
        fs::create_dir_all(&service_dir)?;
        let service_file = service_dir.join("pester.service");
        let content = service_content(&exe);
        fs::write(&service_file, content)
            .with_context(|| format!("failed to write {}", service_file.display()))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run(
            "systemctl",
            &["--user", "enable", "--now", "pester.service"],
        )?;
        term::ok("Installed and started user systemd service.");
        Ok(())
    }

    pub fn uninstall(_paths: &Paths) -> Result<()> {
        let _ = run(
            "systemctl",
            &["--user", "disable", "--now", "pester.service"],
        );
        let service_file = dirs_home()?.join(".config/systemd/user/pester.service");
        if service_file.exists() {
            fs::remove_file(&service_file)?;
        }
        let _ = run("systemctl", &["--user", "daemon-reload"]);
        Ok(())
    }

    pub fn diagnostics(_paths: &Paths) -> Vec<String> {
        let output = Command::new("systemctl")
            .args(["--user", "is-active", "pester.service"])
            .output();
        let Ok(output) = output else {
            return vec![
                "service manager: unavailable (systemctl --user failed to run)".to_string(),
                "service: unknown".to_string(),
            ];
        };
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let status = if status.is_empty() {
            "not installed"
        } else {
            status.as_str()
        };
        vec![
            "service manager: systemd --user".to_string(),
            format!("service: {status}"),
            format!(
                "service file: {}",
                dirs_home()
                    .map(|home| home.join(".config/systemd/user/pester.service"))
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "unknown".to_string())
            ),
        ]
    }

    fn run(program: &str, args: &[&str]) -> Result<()> {
        let status = Command::new(program).args(args).status()?;
        if !status.success() {
            anyhow::bail!("{program} failed with status {status}");
        }
        Ok(())
    }

    fn dirs_home() -> Result<std::path::PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("could not determine home directory")
    }

    fn service_content(exe: &Path) -> String {
        format!(
            "[Unit]\nDescription=pester reminder daemon\n\n[Service]\nExecStart={} system daemon\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            systemd_quote_arg(&exe.display().to_string())
        )
    }

    fn systemd_quote_arg(value: &str) -> String {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => quoted.push_str("\\\\"),
                '"' => quoted.push_str("\\\""),
                '$' => quoted.push_str("$$"),
                '%' => quoted.push_str("%%"),
                _ => quoted.push(ch),
            }
        }
        quoted.push('"');
        quoted
    }

    #[cfg(test)]
    mod tests {
        use std::path::Path;

        use super::{service_content, systemd_quote_arg};

        #[test]
        fn quotes_systemd_exec_start_arguments() {
            assert_eq!(
                systemd_quote_arg("/home/me/pester app/pester"),
                "\"/home/me/pester app/pester\""
            );
            assert_eq!(
                systemd_quote_arg("/home/me/pester\"beta"),
                "\"/home/me/pester\\\"beta\""
            );
            assert_eq!(
                systemd_quote_arg("/home/me/$pester"),
                "\"/home/me/$$pester\""
            );
            assert_eq!(
                systemd_quote_arg("/home/me/%pester"),
                "\"/home/me/%%pester\""
            );
        }

        #[test]
        fn service_file_quotes_executable_path() {
            let content = service_content(Path::new("/home/me/pester app/pester"));

            assert!(content.contains("ExecStart=\"/home/me/pester app/pester\" system daemon"));
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::process::Command;

    use anyhow::{Context, Result};

    use crate::{paths::Paths, term};

    pub fn install(_paths: &Paths) -> Result<()> {
        let exe = daemon_executable()?;
        let launch_agents = home()?.join("Library/LaunchAgents");
        fs::create_dir_all(&launch_agents)?;
        let plist = launch_agents.join("com.aloglu.pester.plist");
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.aloglu.pester</string>
  <key>ProgramArguments</key>
    <array>
    <string>{}</string>
    <string>system</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
"#,
            exe.display()
        );
        fs::write(&plist, content)
            .with_context(|| format!("failed to write {}", plist.display()))?;
        run(
            "launchctl",
            &["load", "-w", plist.to_str().unwrap_or_default()],
        )?;
        term::ok("Installed and started LaunchAgent.");
        Ok(())
    }

    pub fn uninstall(_paths: &Paths) -> Result<()> {
        let plist = home()?.join("Library/LaunchAgents/com.aloglu.pester.plist");
        if plist.exists() {
            let _ = run(
                "launchctl",
                &["unload", "-w", plist.to_str().unwrap_or_default()],
            );
            fs::remove_file(&plist)?;
        }
        let app = home()?.join("Applications/pester.app");
        if app.exists() {
            fs::remove_dir_all(&app)
                .with_context(|| format!("failed to remove {}", app.display()))?;
        }
        Ok(())
    }

    pub fn diagnostics(_paths: &Paths) -> Vec<String> {
        let output = Command::new("launchctl")
            .args(["list", "com.aloglu.pester"])
            .output();
        let Ok(output) = output else {
            return vec![
                "service manager: unavailable (launchctl failed to run)".to_string(),
                "service: unknown".to_string(),
            ];
        };
        let status = if output.status.success() {
            "loaded"
        } else {
            "not installed"
        };
        let home = home().ok();
        let plist = home
            .as_ref()
            .map(|home| home.join("Library/LaunchAgents/com.aloglu.pester.plist"));
        let app = home
            .as_ref()
            .map(|home| home.join("Applications/pester.app"));
        vec![
            "service manager: launchd".to_string(),
            format!("service: {status}"),
            format!(
                "launch agent: {}",
                plist
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            format!(
                "app bundle: {}",
                app.as_ref()
                    .map(|path| {
                        if path.exists() {
                            format!("installed ({})", path.display())
                        } else {
                            format!("missing ({})", path.display())
                        }
                    })
                    .unwrap_or_else(|| "unknown".to_string())
            ),
        ]
    }

    fn run(program: &str, args: &[&str]) -> Result<()> {
        let status = Command::new(program).args(args).status()?;
        if !status.success() {
            anyhow::bail!("{program} failed with status {status}");
        }
        Ok(())
    }

    fn home() -> Result<std::path::PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("could not determine home directory")
    }

    fn daemon_executable() -> Result<std::path::PathBuf> {
        let bundled = home()?.join("Applications/pester.app/Contents/MacOS/pester");
        if bundled.exists() {
            return Ok(bundled);
        }
        std::env::current_exe().context("could not determine current executable")
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ffi::OsStr;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use anyhow::{bail, Context, Result};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
        REG_VALUE_TYPE, RRF_RT_REG_SZ,
    };
    use windows::Win32::System::Threading::{
        OpenEventW, OpenMutexW, ReleaseMutex, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
        MUTEX_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
    };

    use crate::app::APP_NAME;
    use crate::paths::Paths;
    use crate::term;

    const DAEMON_STOP_WAIT_MS: u32 = 5_000;
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn install(_paths: &Paths) -> Result<()> {
        let daemon = daemon_executable()?;
        install_login_startup(&daemon)?;
        if let Err(error) = stop_running_daemon().context("failed to stop existing pester daemon") {
            let _ = remove_login_startup();
            return Err(error);
        }
        if let Err(error) = start_daemon(&daemon) {
            let _ = remove_login_startup();
            return Err(error);
        }
        term::ok("Installed and started Windows login startup.");
        Ok(())
    }

    pub fn uninstall(_paths: &Paths) -> Result<()> {
        let _ = remove_login_startup();
        stop_running_daemon().context("failed to stop pester daemon")?;
        Ok(())
    }

    pub fn diagnostics(_paths: &Paths) -> Vec<String> {
        let login_startup = login_startup_status();
        let expected_startup = daemon_executable()
            .ok()
            .map(|path| command_line_quote_arg(&path.display().to_string()));
        let status = match &login_startup {
            Ok(Some(value)) if expected_startup.as_ref() == Some(value) => "installed",
            Ok(Some(_)) => "installed (different target)",
            Ok(None) => "not installed",
            Err(_) => "unknown",
        };
        vec![
            "service manager: Windows per-user Run key".to_string(),
            format!("service: {status}"),
            format!(
                "login startup: {}",
                run_value_status(login_startup.as_ref())
            ),
        ]
    }

    fn daemon_executable() -> Result<PathBuf> {
        let exe = std::env::current_exe()?;
        let daemon = exe.with_file_name("pesterd.exe");
        if !daemon.exists() {
            bail!(
                "Windows daemon executable is missing: {}. Reinstall pester from a complete Windows artifact.",
                daemon.display()
            );
        }
        Ok(daemon)
    }

    fn start_daemon(daemon: &std::path::Path) -> Result<()> {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        Command::new(daemon)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
            .context("failed to start pester daemon")?;
        Ok(())
    }

    fn stop_running_daemon() -> Result<()> {
        if signal_daemon_stop()? {
            wait_for_daemon_stop()?;
        }
        Ok(())
    }

    fn signal_daemon_stop() -> Result<bool> {
        let handle =
            match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\pester-daemon-stop")) }
            {
                Ok(handle) => handle,
                Err(_) => return Ok(false),
            };

        let result = unsafe { SetEvent(handle) }.context("could not signal pester daemon to stop");
        unsafe {
            let _ = CloseHandle(handle);
        }
        result.map(|_| true)
    }

    fn wait_for_daemon_stop() -> Result<()> {
        let handle = match unsafe {
            OpenMutexW(
                SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE,
                false,
                w!("Local\\pester-daemon"),
            )
        } {
            Ok(handle) => handle,
            Err(_) => return Ok(()),
        };

        let wait_result = unsafe { WaitForSingleObject(handle, DAEMON_STOP_WAIT_MS) };
        let result = if wait_result == WAIT_OBJECT_0 || wait_result == WAIT_ABANDONED {
            unsafe {
                let _ = ReleaseMutex(handle);
            }
            Ok(())
        } else if wait_result == WAIT_TIMEOUT {
            bail!("pester daemon did not stop within 5 seconds")
        } else {
            bail!("failed while waiting for pester daemon to stop: {wait_result:?}")
        };
        unsafe {
            let _ = CloseHandle(handle);
        }
        result
    }

    fn command_line_quote_arg(value: &str) -> String {
        format!("\"{}\"", value.replace('"', "\\\""))
    }

    fn install_login_startup(daemon: &std::path::Path) -> Result<()> {
        let command = command_line_quote_arg(&daemon.display().to_string());
        let subkey = wide_null(RUN_KEY);
        let name = wide_null(APP_NAME);
        let data = wide_null(&command);
        let byte_len = (data.len() * std::mem::size_of::<u16>()) as u32;

        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                byte_len,
            )
            .ok()
            .context("failed to install pester login startup entry")?;
        }
        Ok(())
    }

    fn remove_login_startup() -> Result<()> {
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;

        let subkey = wide_null(RUN_KEY);
        let name = wide_null(APP_NAME);
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status
            .ok()
            .context("failed to remove pester login startup entry")
    }

    fn login_startup_status() -> Result<Option<String>> {
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;

        let subkey = wide_null(RUN_KEY);
        let name = wide_null(APP_NAME);
        let mut value_type = REG_VALUE_TYPE::default();
        let mut byte_len = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                Some(&mut value_type),
                None,
                Some(&mut byte_len),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status
            .ok()
            .context("failed to read pester login startup entry")?;
        if value_type != REG_SZ {
            return Ok(None);
        }

        let mut buffer = vec![0u16; (byte_len as usize + 1) / std::mem::size_of::<u16>()];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                Some(&mut value_type),
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut byte_len),
            )
        };
        status
            .ok()
            .context("failed to read pester login startup entry")?;
        let len = buffer
            .iter()
            .position(|ch| *ch == 0)
            .unwrap_or(buffer.len());
        Ok(Some(String::from_utf16_lossy(&buffer[..len])))
    }

    fn run_value_status(value: std::result::Result<&Option<String>, &anyhow::Error>) -> String {
        match value {
            Ok(Some(value)) => format!("installed ({value})"),
            Ok(None) => "missing".to_string(),
            Err(_) => "unknown".to_string(),
        }
    }

    fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(iter::once(0)).collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use anyhow::{bail, Result};

    use crate::paths::Paths;

    pub fn install(_paths: &Paths) -> Result<()> {
        bail!("service installation is only supported on Linux, macOS, and Windows")
    }

    pub fn uninstall(_paths: &Paths) -> Result<()> {
        Ok(())
    }

    pub fn diagnostics(_paths: &Paths) -> Vec<String> {
        vec!["service: unsupported platform".to_string()]
    }
}
