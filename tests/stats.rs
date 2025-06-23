//! Integration test for table and database statistics using redb's stats() and len().
//!
//! This test demonstrates how to access and validate table statistics and entry count.

use redb::{ReadableTableMetadata, TableDefinition};
use tempfile::NamedTempFile;

const TABLE: TableDefinition<u64, u64> = TableDefinition::new("stats");

#[test]
fn test_table_statistics() {
    let tmpfile = NamedTempFile::new().unwrap();
    let db = redb::Builder::new()
        .create_with_file_format_v3(true)
        .create(tmpfile.path())
        .unwrap();

    // Insert some entries
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(TABLE).unwrap();
        for i in 0..10u64 {
            table.insert(&i, &i).unwrap();
        }
    }
    write_txn.commit().unwrap();

    // Read stats
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    let stats = table.stats().unwrap();
    let len = table.len().unwrap();
    assert_eq!(len, 10);
    // Print stats for manual inspection (optional)
    println!("Table stats: {:?}", stats);
}
