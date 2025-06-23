// This test demonstrates redb's multimap table feature:
// - A multimap table allows multiple values for a single key.
// - Shows insertion and retrieval of multiple values for the same key.
//
// This is useful for use-cases where a key can have a set of associated values.

use redb::{Database, MultimapTableDefinition};

const MULTIMAP: MultimapTableDefinition<&str, u64> = MultimapTableDefinition::new("test_multimap");

fn temp_db() -> Database {
    let file = tempfile::NamedTempFile::new().unwrap();
    redb::Builder::new()
        .create_with_file_format_v3(true)
        .create(file.path())
        .unwrap()
}

#[test]
fn test_multimap_insert_and_read() {
    let db = temp_db();
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut mmap = write_txn.open_multimap_table(MULTIMAP).unwrap();
            mmap.insert("group", &1).unwrap();
            mmap.insert("group", &2).unwrap();
        }
        write_txn.commit().unwrap();
    }
    let read_txn = db.begin_read().unwrap();
    let mmap = read_txn.open_multimap_table(MULTIMAP).unwrap();
    let values: Vec<_> = mmap
        .get("group")
        .unwrap()
        .map(|v| v.unwrap().value())
        .collect();
    assert_eq!(values, vec![1, 2]);
}
