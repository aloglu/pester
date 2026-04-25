use windows::core::{w, PCWSTR};

pub fn daemon_mutex_name() -> PCWSTR {
    w!("Local\\pester-daemon")
}

pub fn daemon_stop_event_name() -> PCWSTR {
    w!("Local\\pester-daemon-stop")
}
