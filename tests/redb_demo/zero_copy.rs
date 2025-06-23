//! Demonstration test for zero-copy/in-place mutation using redb's insert_reserve and MutInPlaceValue.
//!
//! This test demonstrates how to use insert_reserve to perform zero-copy/in-place mutation
//! with a value type that implements MutInPlaceValue (e.g., [u8; N]).

use redb::TableDefinition;
use tempfile::NamedTempFile;

const TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("zero_copy");

#[test]
fn test_zero_copy_in_place_mutation() {
    // Create a temporary database file
    let tmpfile = NamedTempFile::new().unwrap();
    let db = redb::Builder::new()
        .create_with_file_format_v3(true)
        .create(tmpfile.path())
        .unwrap();

    // Start a write transaction
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TABLE).unwrap();
        // Reserve space for a value of 8 bytes
        let mut buf = table.insert_reserve(&1u64, 8).unwrap();
        buf.as_mut().copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    }
    write_txn.commit().unwrap();

    // Read back the value
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    let value_guard = table.get(&1u64).unwrap().unwrap();
    let value = value_guard.value();
    assert_eq!(value, &[1, 2, 3, 4, 5, 6, 7, 8]);
}
