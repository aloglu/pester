#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::Result;
use pester::store::Store;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let Some(_instance) = SingleInstance::acquire()? else {
        return Ok(());
    };

    pester::daemon::run(Store::new()?)
}

#[cfg(target_os = "windows")]
struct SingleInstance(windows::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl SingleInstance {
    fn acquire() -> Result<Option<Self>> {
        use windows::core::w;
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;

        use anyhow::Context as _;

        let handle = unsafe { CreateMutexW(None, true, w!("Local\\pester-daemon")) }
            .context("could not create pester daemon instance mutex")?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Ok(None);
        }

        Ok(Some(Self(handle)))
    }
}

#[cfg(target_os = "windows")]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct SingleInstance;

#[cfg(not(target_os = "windows"))]
impl SingleInstance {
    fn acquire() -> Result<Option<Self>> {
        Ok(Some(Self))
    }
}
