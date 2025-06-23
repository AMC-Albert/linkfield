// This test demonstrates redb's savepoint and rollback feature:
// - Shows how to create an ephemeral savepoint in a write transaction
// - Demonstrates rolling back to a savepoint to undo changes
//
// This is useful for implementing partial rollbacks or complex transactional logic.

use redb::{Database, TableDefinition};

const TABLE: TableDefinition<&str, u64> = TableDefinition::new("test_table");

fn temp_db() -> Database {
    let file = tempfile::NamedTempFile::new().unwrap();
    Database::create(file.path()).unwrap()
}

#[test]
fn test_savepoint_and_rollback() {
    let db = temp_db();
    let mut write_txn = db.begin_write().unwrap();
    let savepoint = write_txn.ephemeral_savepoint().unwrap();
    {
        let mut table = write_txn.open_table(TABLE).unwrap();
        table.insert("foo", &1).unwrap();
    }
    write_txn.restore_savepoint(&savepoint).unwrap();
    {
        let mut table = write_txn.open_table(TABLE).unwrap();
        table.insert("bar", &2).unwrap();
    }
    write_txn.commit().unwrap();
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(TABLE).unwrap();
    assert!(table.get("foo").unwrap().is_none());
    assert_eq!(table.get("bar").unwrap().unwrap().value(), 2);
}
