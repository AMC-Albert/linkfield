use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use tracing::{info, info_span};
use windows::Win32::System::Registry::{
	HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
	RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::PCWSTR;

fn to_wide(s: &str) -> Vec<u16> {
	OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
pub fn register_redb_extension(_all_users: bool) -> std::io::Result<()> {
	let span = info_span!("register_redb_extension");
	let _enter = span.enter();

	let exe_path = std::env::current_exe()?;
	let exe_path_str = exe_path
		.to_str()
		.ok_or_else(|| std::io::Error::other("Executable path contains invalid UTF-8"))?
		.to_string();
	let prog_id = "Linkfield.redb";
	let friendly_name = "Linkfield Database File";
	let hkcu = HKEY_CURRENT_USER;

	// .redb extension
	set_registry_value(hkcu, r"Software\Classes\.redb", prog_id);
	// ProgID
	set_registry_value(hkcu, &format!(r"Software\Classes\{prog_id}"), friendly_name);
	// shell\open\command
	set_registry_value(
		hkcu,
		&format!(r"Software\Classes\{prog_id}\shell\open\command"),
		&format!("\"{exe_path_str}\" \"%1\""),
	);
	// DefaultIcon
	set_registry_value(
		hkcu,
		&format!(r"Software\Classes\{prog_id}\DefaultIcon"),
		&format!("\"{exe_path_str}\",0"),
	);
	notify_shell_assoc_changed();
	info!(exe_path = %exe_path_str, "Registered .redb extension");
	Ok(())
}

fn set_registry_value(hkey: windows::Win32::System::Registry::HKEY, path: &str, value: &str) {
	let span = info_span!("set_registry_value", path = path, value = value);
	let _enter = span.enter();
	let to_wide = |s: &str| {
		OsStr::new(s)
			.encode_wide()
			.chain(Some(0))
			.collect::<Vec<u16>>()
	};
	unsafe {
		let key_path = to_wide(path);
		let mut hkey_out = windows::Win32::System::Registry::HKEY::default();
		let _ = RegCreateKeyExW(
			hkey,
			PCWSTR(key_path.as_ptr()),
			None,
			None,
			REG_OPTION_NON_VOLATILE,
			KEY_SET_VALUE,
			None,
			&mut hkey_out,
			None,
		);
		let value_wide = to_wide(value);
		let _ = RegSetValueExW(
			hkey_out,
			None,
			None,
			REG_SZ,
			Some(std::slice::from_raw_parts(
				value_wide.as_ptr().cast::<u8>(),
				value_wide.len() * 2,
			)),
		);
		let _ = RegCloseKey(hkey_out);
	}
}

fn notify_shell_assoc_changed() {
	let span = info_span!("notify_shell_assoc_changed");
	let _enter = span.enter();
	unsafe {
		SHChangeNotify(
			SHCNE_ASSOCCHANGED,
			SHCNF_IDLIST,
			Some(std::ptr::null()),
			Some(std::ptr::null()),
		);
	}
}

#[cfg(windows)]
pub fn is_redb_registered() -> bool {
	let span = info_span!("is_redb_registered");
	let _enter = span.enter();

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
			let mut buf_len = (buf.len() * 2).try_into().unwrap_or(u32::MAX);
			if RegQueryValueExW(
				hkey,
				None,
				None,
				None,
				Some(buf.as_mut_ptr().cast::<u8>()),
				Some(&mut buf_len),
			)
			.is_ok()
			{
				let val =
					String::from_utf16_lossy(&buf[..(buf_len as usize / 2).saturating_sub(1)]);
				let _ = RegCloseKey(hkey);
				return val == prog_id;
			}
			let _ = RegCloseKey(hkey);
		}
	}
	false
}
