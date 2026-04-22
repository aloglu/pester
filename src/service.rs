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
            "[Unit]\nDescription=Pester reminder daemon\n\n[Service]\nExecStart={} daemon\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
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
                systemd_quote_arg("/home/me/Pester App/pester"),
                "\"/home/me/Pester App/pester\""
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
            let content = service_content(Path::new("/home/me/Pester App/pester"));

            assert!(content.contains("ExecStart=\"/home/me/Pester App/pester\" daemon"));
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
        let app = home()?.join("Applications/Pester.app");
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
            .map(|home| home.join("Applications/Pester.app"));
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
        let bundled = home()?.join("Applications/Pester.app/Contents/MacOS/pester");
        if bundled.exists() {
            return Ok(bundled);
        }
        std::env::current_exe().context("could not determine current executable")
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use anyhow::{Context, Result};
    use windows::core::{Interface, HSTRING, PROPVARIANT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, PROPERTYKEY};
    use windows::Win32::UI::Shell::{
        FOLDERID_StartMenu, FOLDERID_Startup, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SHOW_WINDOW_CMD, SW_HIDE, SW_SHOWNORMAL};

    use crate::app::{APP_ID, APP_NAME};
    use crate::{paths::Paths, term};

    pub fn install(_paths: &Paths) -> Result<()> {
        let exe = std::env::current_exe()?;
        create_start_menu_shortcut(&exe)?;
        match install_scheduled_task(&exe) {
            Ok(()) => term::ok("Installed and started Scheduled Task."),
            Err(error) => {
                let _ = run("schtasks", &["/Delete", "/TN", "Pester", "/F"]);
                create_startup_shortcut(&exe)?;
                start_daemon(&exe)?;
                term::warn(format!("Task Scheduler setup failed ({error:#})."));
                term::ok("Installed and started Startup shortcut fallback.");
            }
        }
        Ok(())
    }

    pub fn uninstall(_paths: &Paths) -> Result<()> {
        let _ = run("schtasks", &["/End", "/TN", "Pester"]);
        let _ = run("schtasks", &["/Delete", "/TN", "Pester", "/F"]);
        let _ = stop_daemon_processes();
        let _ = remove_start_menu_shortcut();
        let _ = remove_startup_shortcut();
        Ok(())
    }

    pub fn diagnostics(_paths: &Paths) -> Vec<String> {
        let output = Command::new("schtasks")
            .args(["/Query", "/TN", "Pester"])
            .output();
        let start_menu_shortcut = start_menu_shortcut_path();
        let startup_shortcut = startup_shortcut_path();
        let startup_installed = startup_shortcut
            .as_ref()
            .map(|path| path.exists())
            .unwrap_or(false);
        let (manager, status) = match output {
            Ok(output) => {
                let task_installed = output.status.success();
                let status = match (task_installed, startup_installed) {
                    (true, true) => "installed (Task Scheduler and Startup shortcut fallback)",
                    (true, false) => "installed (Task Scheduler)",
                    (false, true) => "installed (Startup shortcut fallback)",
                    (false, false) => "not installed",
                };
                ("Windows Task Scheduler", status)
            }
            Err(_) => (
                "unavailable (schtasks failed to run)",
                if startup_installed {
                    "installed (Startup shortcut fallback)"
                } else {
                    "unknown"
                },
            ),
        };
        vec![
            format!("service manager: {manager}"),
            format!("service: {status}"),
            format!(
                "start menu shortcut: {}",
                shortcut_status(start_menu_shortcut.as_ref())
            ),
            format!(
                "startup shortcut fallback: {}",
                shortcut_status(startup_shortcut.as_ref())
            ),
        ]
    }

    fn install_scheduled_task(exe: &std::path::Path) -> Result<()> {
        let task = format!("\"{}\" daemon", exe.display());
        run(
            "schtasks",
            &[
                "/Create", "/TN", "Pester", "/SC", "ONLOGON", "/TR", &task, "/F",
            ],
        )?;
        run("schtasks", &["/Run", "/TN", "Pester"])?;
        Ok(())
    }

    fn start_daemon(exe: &std::path::Path) -> Result<()> {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        Command::new(exe)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
            .context("failed to start Pester daemon")?;
        Ok(())
    }

    fn stop_daemon_processes() -> Result<()> {
        let current_pid = std::process::id();
        let script = format!(
            "$CurrentPid = {current_pid}; \
             Get-CimInstance Win32_Process -Filter \"Name = 'pester.exe'\" | \
             Where-Object {{ $_.ProcessId -ne $CurrentPid -and $_.CommandLine -like '* daemon*' }} | \
             ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
        );
        run("powershell", &["-NoProfile", "-Command", &script])
    }

    fn run(program: &str, args: &[&str]) -> Result<()> {
        let output = Command::new(program).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = stderr.trim();
            let detail = if detail.is_empty() {
                stdout.trim()
            } else {
                detail
            };
            if detail.is_empty() {
                anyhow::bail!("{program} failed with status {}", output.status);
            }
            anyhow::bail!("{program} failed with status {}: {detail}", output.status);
        }
        Ok(())
    }

    fn create_start_menu_shortcut(exe: &std::path::Path) -> Result<()> {
        create_shortcut(
            &start_menu_shortcut_path()?,
            exe,
            "daemon",
            "Pester reminder daemon",
            SW_SHOWNORMAL,
        )
    }

    fn create_startup_shortcut(exe: &std::path::Path) -> Result<()> {
        let (target, arguments) = hidden_startup_command(exe);
        create_shortcut(
            &startup_shortcut_path()?,
            &target,
            &arguments,
            "Start Pester reminder daemon at login",
            SW_HIDE,
        )
    }

    fn hidden_startup_command(exe: &std::path::Path) -> (PathBuf, String) {
        let exe = powershell_single_quoted(&exe.display().to_string());
        let command =
            format!("Start-Process -WindowStyle Hidden -FilePath {exe} -ArgumentList 'daemon'");
        (
            powershell_path(),
            format!(
                "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"{command}\""
            ),
        )
    }

    fn powershell_path() -> PathBuf {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .map(|root| {
                root.join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            })
            .unwrap_or_else(|| PathBuf::from("powershell.exe"))
    }

    fn powershell_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn create_shortcut(
        shortcut_path: &std::path::Path,
        exe: &std::path::Path,
        arguments: &str,
        description: &str,
        show_command: SHOW_WINDOW_CMD,
    ) -> Result<()> {
        let _com = ComApartment::new()?;
        if let Some(parent) = shortcut_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        unsafe {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("could not create Windows ShellLink COM object")?;
            shell_link
                .SetPath(&HSTRING::from(exe.display().to_string()))
                .context("could not set Pester shortcut path")?;
            shell_link
                .SetArguments(&HSTRING::from(arguments))
                .context("could not set Pester shortcut arguments")?;
            shell_link
                .SetDescription(&HSTRING::from(description))
                .context("could not set Pester shortcut description")?;
            shell_link
                .SetShowCmd(show_command)
                .context("could not set Pester shortcut show command")?;

            let property_store: IPropertyStore = shell_link
                .cast()
                .context("could not access Pester shortcut property store")?;
            set_app_user_model_id(&property_store)?;
            property_store
                .Commit()
                .context("could not commit Pester shortcut properties")?;

            let persist_file: IPersistFile = shell_link
                .cast()
                .context("could not access Pester shortcut persistence")?;
            persist_file
                .Save(&HSTRING::from(shortcut_path.display().to_string()), true)
                .context("could not save Pester shortcut")?;
        }

        Ok(())
    }

    fn remove_start_menu_shortcut() -> Result<()> {
        let shortcut_path = start_menu_shortcut_path()?;
        remove_shortcut(&shortcut_path)
    }

    fn remove_startup_shortcut() -> Result<()> {
        let shortcut_path = startup_shortcut_path()?;
        remove_shortcut(&shortcut_path)
    }

    fn remove_shortcut(shortcut_path: &std::path::Path) -> Result<()> {
        if shortcut_path.exists() {
            std::fs::remove_file(shortcut_path)
                .with_context(|| format!("failed to remove {}", shortcut_path.display()))?;
        }
        Ok(())
    }

    fn start_menu_shortcut_path() -> Result<PathBuf> {
        let start_menu = known_folder_path(&FOLDERID_StartMenu)?;
        Ok(start_menu
            .join("Programs")
            .join(APP_NAME)
            .join(format!("{APP_NAME}.lnk")))
    }

    fn startup_shortcut_path() -> Result<PathBuf> {
        Ok(known_folder_path(&FOLDERID_Startup)?.join(format!("{APP_NAME}.lnk")))
    }

    fn shortcut_status(path: std::result::Result<&PathBuf, &anyhow::Error>) -> String {
        match path {
            Ok(path) if path.exists() => format!("installed ({})", path.display()),
            Ok(path) => format!("missing ({})", path.display()),
            Err(_) => "unknown".to_string(),
        }
    }

    fn known_folder_path(folder_id: &windows::core::GUID) -> Result<PathBuf> {
        unsafe {
            let path = SHGetKnownFolderPath(folder_id, Default::default(), None)
                .context("could not locate Windows Start Menu folder")?;
            let path_string = path
                .to_string()
                .context("Start Menu path is not valid UTF-16")?;
            CoTaskMemFree(Some(path.as_ptr().cast()));
            Ok(PathBuf::from(path_string))
        }
    }

    unsafe fn set_app_user_model_id(property_store: &IPropertyStore) -> Result<()> {
        const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
            pid: 5,
        };

        let value = PROPVARIANT::from(APP_ID);
        property_store
            .SetValue(&PKEY_APP_USER_MODEL_ID, &value)
            .context("could not set Pester AppUserModelID")
    }

    struct ComApartment;

    impl ComApartment {
        fn new() -> Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .context("could not initialize COM apartment")?;
            }
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
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
