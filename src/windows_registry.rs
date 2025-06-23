#[cfg(windows)]
pub fn register_redb_extension(_all_users: bool) -> std::io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    let exe_path = std::env::current_exe()?.to_str().unwrap().to_string();
    let prog_id = "Linkfield.redb";
    let friendly_name = "Linkfield Database File";
    let hkcu = HKEY_CURRENT_USER;

    unsafe {
        // .redb extension
        let key_path = to_wide(r"Software\Classes\.redb");
        let mut hkey = HKEY::default();
        let _ = RegCreateKeyExW(
            hkcu,
            PCWSTR(key_path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        let prog_id_wide = to_wide(prog_id);
        let _ = RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                prog_id_wide.as_ptr() as *const u8,
                prog_id_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);

        // ProgID
        let progid_path = to_wide(&format!(r"Software\Classes\{}", prog_id));
        let mut hkey = HKEY::default();
        let _ = RegCreateKeyExW(
            hkcu,
            PCWSTR(progid_path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        let friendly_name_wide = to_wide(friendly_name);
        let _ = RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                friendly_name_wide.as_ptr() as *const u8,
                friendly_name_wide.len() * 2,
            )),
        );

        // shell\open\command
        let shell_path = to_wide(r"shell\open\command");
        let mut shell_key = HKEY::default();
        let _ = RegCreateKeyExW(
            hkey,
            PCWSTR(shell_path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut shell_key,
            None,
        );
        let command = format!("\"{}\" \"%1\"", exe_path);
        let command_wide = to_wide(&command);
        let _ = RegSetValueExW(
            shell_key,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                command_wide.as_ptr() as *const u8,
                command_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(shell_key);

        // DefaultIcon
        let icon_path = to_wide("DefaultIcon");
        let mut icon_key = HKEY::default();
        let _ = RegCreateKeyExW(
            hkey,
            PCWSTR(icon_path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut icon_key,
            None,
        );
        let icon_val = format!("\"{}\",0", exe_path);
        let icon_val_wide = to_wide(&icon_val);
        let _ = RegSetValueExW(
            icon_key,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                icon_val_wide.as_ptr() as *const u8,
                icon_val_wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(icon_key);
        let _ = RegCloseKey(hkey);
    }
    println!(".redb extension registered to {}", exe_path);
    Ok(())
}

#[cfg(windows)]
pub fn is_redb_registered() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::PCWSTR;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    let prog_id = "Linkfield.redb";
    let hkcu = HKEY_CURRENT_USER;
    unsafe {
        let key_path = to_wide(r"Software\Classes\.redb");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            hkcu,
            PCWSTR(key_path.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
        .is_ok()
        {
            let mut buf = [0u16; 128];
            let mut buf_len = (buf.len() * 2) as u32;
            if RegQueryValueExW(
                hkey,
                None,
                None,
                None,
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut buf_len),
            )
            .is_ok()
            {
                let val = String::from_utf16_lossy(&buf[..(buf_len as usize / 2) - 1]);
                let _ = RegCloseKey(hkey);
                return val == prog_id;
            }
            let _ = RegCloseKey(hkey);
        }
    }
    false
}
