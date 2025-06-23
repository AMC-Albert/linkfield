// Platform-specific logic (Windows registry, exit handling, etc.)

#[cfg(windows)]
pub fn handle_platform_startup() {
    use crate::windows_registry::{is_redb_registered, register_redb_extension};
    if !is_redb_registered() {
        if let Err(e) = register_redb_extension(false) {
            tracing::error!(error = %e, "Failed to register .redb extension");
        }
    }
}

#[cfg(not(windows))]
pub fn handle_platform_startup() {}

pub fn wait_for_exit() {
    use std::io::{self, Read};
    tracing::info!("Press Enter to exit...");
    let stdin = io::stdin();
    let mut buf = [0u8; 1];
    loop {
        let read_result = {
            let mut handle = stdin.lock();
            handle.read(&mut buf)
        };
        match read_result {
            Ok(n) if n > 0 && buf[0] == b'\n' => break,
            Ok(_) => (),
            Err(e) => {
                tracing::error!(error = %e, "stdin read failed");
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
