// Platform-specific logic (Windows registry, exit handling, etc.)

#[cfg(windows)]
pub fn handle_platform_startup() {
    use crate::windows_registry::{is_redb_registered, register_redb_extension};
    if !is_redb_registered() {
        if let Err(e) = register_redb_extension(false) {
            eprintln!("[main] Failed to register .redb extension: {e}");
        }
    }
}

#[cfg(not(windows))]
pub fn handle_platform_startup() {}

pub fn wait_for_exit() {
    use std::io::{self, Read};
    println!("\nPress Enter to exit...");
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut buf = [0u8; 1];
    loop {
        match handle.read(&mut buf) {
            Ok(n) if n > 0 && buf[0] == b'\n' => break,
            Ok(_) => (),
            Err(e) => {
                eprintln!("[main] ERROR: stdin read failed: {e}");
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
