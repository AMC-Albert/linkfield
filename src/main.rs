use redb::Builder;
use std::env;

#[cfg(windows)]
fn register_redb_extension(all_users: bool) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;
    let exe_path = env::current_exe()?.to_str().unwrap().to_string();
    let prog_id = "Linkfield.redb";
    let friendly_name = "Linkfield Database File";
    let root = if all_users {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    };
    // Set .redb default value to our ProgID
    let (redb_key, _) = root.create_subkey(r"Software\Classes\.redb")?;
    redb_key.set_value("", &prog_id)?;
    // Register ProgID
    let (progid_key, _) = root.create_subkey(format!(r"Software\\Classes\\{}", prog_id))?;
    progid_key.set_value("", &friendly_name)?;
    let (shell_key, _) = progid_key.create_subkey(r"shell\open\command")?;
    shell_key.set_value("", &format!("\"{}\" \"%1\"", exe_path))?;
    // Set the icon for .redb files to the program's own icon
    let (default_icon_key, _) = progid_key.create_subkey("DefaultIcon")?;
    default_icon_key.set_value("", &format!("\"{}\",0", exe_path))?;
    println!(".redb extension registered to {}", exe_path);
    Ok(())
}

fn main() {
    #[cfg(windows)]
    {
        // Register for current user. Set to true for all users (requires admin)
        if let Err(e) = register_redb_extension(false) {
            eprintln!("Failed to register .redb extension: {}", e);
        }
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let db_path = &args[1];
        println!("Opened via double-click or 'Open with': {}", db_path);
        match Builder::new().open(db_path) {
            Ok(db) => {
                // Try to read the test_table and print the value for 'hello'
                const TABLE: redb::TableDefinition<&str, u64> =
                    redb::TableDefinition::new("test_table");
                let read_txn = db.begin_read().unwrap();
                let table = read_txn.open_table(TABLE);
                match table {
                    Ok(table) => match table.get("hello") {
                        Ok(Some(val)) => println!("Found: ('hello', {})", val.value()),
                        Ok(None) => println!("Key 'hello' not found in 'test_table'."),
                        Err(e) => println!("Error reading from table: {}", e),
                    },
                    Err(e) => println!("Could not open 'test_table': {}", e),
                }
            }
            Err(e) => println!("Failed to open database: {}", e),
        }
    } else {
        // Create a simple test database and insert a value
        let db = Builder::new()
            .create_with_file_format_v3(true)
            .create("test.redb")
            .expect("Failed to create test.redb");
        const TABLE: redb::TableDefinition<&str, u64> = redb::TableDefinition::new("test_table");
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            table.insert("hello", &42).unwrap();
        }
        write_txn.commit().unwrap();
        println!("Created test.redb and inserted ('hello', 42) into 'test_table'.");
    }

    // Keep the console window open if launched by double-click
    #[cfg(windows)]
    {
        use std::io::{self, Write};
        print!("\nPress Enter to exit...");
        io::stdout().flush().ok();
        let _ = io::stdin().read_line(&mut String::new());
    }
}
