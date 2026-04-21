#[cfg(any(target_os = "windows", test))]
pub const APP_ID: &str = "com.aloglu.pester";
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub const APP_NAME: &str = "Pester";
