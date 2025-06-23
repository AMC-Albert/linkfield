// This test demonstrates large batch transactions in redb:
// - Inserts a large number of key-value pairs in a single write transaction
// - Verifies that all values are present after the commit
//
// This is useful for bulk loading and performance-sensitive workloads.

use redb::{Database, ReadableTableMetadata, TableDefinition};

const TABLE: TableDefinition<u32, &[u8]> = TableDefinition::new("batch_table");

fn temp_db() -> Database {
    let file = tempfile::NamedTempFile::new().unwrap();
    redb::Builder::new()
        .create_with_file_format_v3(true)
        .create(file.path())
        .unwrap()
}

#[test]
fn test_large_batch_transaction() {
    let db = temp_db();
    let batch_size: u32 = 10_000;
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            for i in 0..batch_size {
                let value = i.to_le_bytes();
                table.insert(i, &value[..]).unwrap();
            }
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    for i in 0..batch_size {
        let expected = i.to_le_bytes();
        let result = table.get(i).unwrap().unwrap();
        assert_eq!(result.value(), &expected[..]);
    }
}

#[test]
fn test_zero_copy_and_in_place_mutation() {
    // Demonstrates zero-copy/in-place mutation using insert_reserve
    let db = temp_db();
    let key = 42u32;
    let value = [100u8, 0, 0, 0];
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            let mut buf = table.insert_reserve(key, 4).unwrap();
            buf.as_mut().copy_from_slice(&value);
            // Mutate in-place: increment the first byte
            buf.as_mut()[0] += 1;
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    let result = table.get(key).unwrap().unwrap();
    assert_eq!(result.value(), &[101, 0, 0, 0]);
}

#[test]
fn test_table_and_database_stats() {
    let db = temp_db();
    let batch_size: u32 = 100;
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            for i in 0..batch_size {
                let value = [i as u8, 0, 0, 0];
                table.insert(i, &value[..]).unwrap();
            }
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    let len = table.len().unwrap();
    assert_eq!(len, batch_size as u64);
    // DatabaseStats is not available on ReadTransaction in redb 2.6.0, so we skip it here.
}
