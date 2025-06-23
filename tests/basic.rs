// This test suite demonstrates basic redb features:
// - Basic insert and read operations with string keys and integer values
// - Use of custom key and value types (byte slices and arrays)
//   to show redb's flexibility with types that implement Key/Value traits
//
// These tests help verify that redb can:
// - Store and retrieve simple and complex types
// - Work with zero-copy, efficient data representations

use redb::{Database, TableDefinition};

const TABLE: TableDefinition<&str, u64> = TableDefinition::new("test_table");
const TABLE_BYTES: TableDefinition<&[u8], [u8; 4]> = TableDefinition::new("bytes_table");

fn temp_db() -> Database {
    let file = tempfile::NamedTempFile::new().unwrap();
    Database::create(file.path()).unwrap()
}

#[test]
fn test_basic_insert_and_read() {
    let db = temp_db();
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            table.insert("foo", &123).unwrap();
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    assert_eq!(table.get("foo").unwrap().unwrap().value(), 123);
}

#[test]
fn test_custom_key_value_types() {
    let db = temp_db();
    let key: &[u8] = b"abcd";
    let value: [u8; 4] = [1, 2, 3, 4];
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE_BYTES).unwrap();
            table.insert(key, &value).unwrap();
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE_BYTES).unwrap();
    let result = table.get(key).unwrap().unwrap().value();
    assert_eq!(result, value);
}
