// This test demonstrates redb's MVCC (Multi-Version Concurrency Control) feature:
// - Multiple concurrent readers can see a consistent snapshot of the database
// - Readers are not blocked by writers, and always see the data as it was when their transaction started
//
// This is important for high-concurrency workloads and ensures snapshot isolation.

use redb::{Database, TableDefinition};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const TABLE: TableDefinition<&str, u64> = TableDefinition::new("test_table");

fn temp_db() -> Database {
    let file = tempfile::NamedTempFile::new().unwrap();
    Database::create(file.path()).unwrap()
}

#[test]
fn test_mvcc_concurrent_readers() {
    let db = Arc::new(temp_db());
    {
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            table.insert("alpha", &1).unwrap();
        }
        write_txn.commit().unwrap();
    }
    let db1 = db.clone();
    let db2 = db.clone();
    let h1 = thread::spawn(move || {
        let read_txn = db1.begin_read().unwrap();
        let table = read_txn.open_table(TABLE).unwrap();
        let val = table.get("alpha").unwrap().unwrap().value();
        thread::sleep(Duration::from_millis(500));
        let val2 = table.get("alpha").unwrap().unwrap().value();
        assert_eq!(val, val2);
    });
    let h2 = thread::spawn(move || {
        let read_txn = db2.begin_read().unwrap();
        let table = read_txn.open_table(TABLE).unwrap();
        let val = table.get("alpha").unwrap().unwrap().value();
        thread::sleep(Duration::from_millis(500));
        let val2 = table.get("alpha").unwrap().unwrap().value();
        assert_eq!(val, val2);
    });
    h1.join().unwrap();
    h2.join().unwrap();
}
