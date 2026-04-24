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

    #[cfg(target_os = "windows")]
    {
        let stop_event = StopEvent::create()?;
        pester::daemon::run_with_shutdown(Store::new()?, |duration| stop_event.wait(duration))
    }

    #[cfg(not(target_os = "windows"))]
    {
        pester::daemon::run(Store::new()?)
    }
}

#[cfg(target_os = "windows")]
struct SingleInstance(windows::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl SingleInstance {
    fn acquire() -> Result<Option<Self>> {
        use windows::core::w;
        use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;

        use anyhow::Context as _;

        let handle = unsafe { CreateMutexW(None, true, w!("Local\\pester-daemon")) }
            .context("could not create pester daemon instance mutex")?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }

        Ok(Some(Self(handle)))
    }
}

#[cfg(target_os = "windows")]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(self.0);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
struct StopEvent(windows::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl StopEvent {
    fn create() -> Result<Self> {
        use windows::core::w;
        use windows::Win32::System::Threading::{CreateEventW, ResetEvent};

        use anyhow::Context as _;

        let handle = unsafe { CreateEventW(None, true, false, w!("Local\\pester-daemon-stop")) }
            .context("could not create pester daemon stop event")?;
        unsafe {
            ResetEvent(handle).context("could not reset pester daemon stop event")?;
        }
        Ok(Self(handle))
    }

    fn wait(&self, duration: std::time::Duration) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::WaitForSingleObject;

        let millis = duration.as_millis().try_into().unwrap_or(u32::MAX);
        unsafe { WaitForSingleObject(self.0, millis) == WAIT_OBJECT_0 }
    }
}

#[cfg(target_os = "windows")]
impl Drop for StopEvent {
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
