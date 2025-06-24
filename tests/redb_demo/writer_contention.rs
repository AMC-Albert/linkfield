// This test suite demonstrates redb's concurrency and writer contention features:
// - Only one write transaction can exist at a time (exclusive writer lock)
// - Tests for rapid-fire writer attempts and timing of lock acquisition
// - Readers see consistent snapshots even during rapid writes
//
// These tests help verify redb's ACID guarantees and concurrency model.

use redb::{Database, TableDefinition};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

fn temp_db() -> Database {
	let file = tempfile::NamedTempFile::new().unwrap();
	redb::Builder::new()
		.create_with_file_format_v3(true)
		.create(file.path())
		.unwrap()
}

#[test]
fn test_writer_contention() {
	let db = Arc::new(temp_db());
	let barrier = Arc::new(Barrier::new(2));
	let db_writer1 = db.clone();
	let barrier_writer1 = barrier.clone();
	let handle1 = thread::spawn(move || {
		let write_txn = db_writer1.begin_write().unwrap();
		barrier_writer1.wait();
		thread::sleep(Duration::from_millis(500));
		drop(write_txn);
	});
	let db_writer2 = db.clone();
	let barrier_writer2 = barrier.clone();
	let handle2 = thread::spawn(move || {
		barrier_writer2.wait();
		match db_writer2.begin_write() {
			Ok(_txn) => (),
			Err(e) => panic!("Writer 2 failed to acquire write transaction: {}", e),
		}
	});
	handle1.join().unwrap();
	handle2.join().unwrap();
}

#[test]
fn test_writer_contention_with_timeout() {
	let db = Arc::new(temp_db());
	let barrier = Arc::new(Barrier::new(2));
	let db_writer1 = db.clone();
	let barrier_writer1 = barrier.clone();
	let handle1 = thread::spawn(move || {
		let write_txn = db_writer1.begin_write().unwrap();
		barrier_writer1.wait();
		thread::sleep(Duration::from_secs(2));
		drop(write_txn);
	});
	let db_writer2 = db.clone();
	let barrier_writer2 = barrier.clone();
	let handle2 = thread::spawn(move || {
		barrier_writer2.wait();
		let start = std::time::Instant::now();
		let _write_txn = db_writer2.begin_write().unwrap();
		let elapsed = start.elapsed();
		// Should have waited at least 2 seconds for the lock
		assert!(
			elapsed >= Duration::from_secs(2),
			"Writer 2 did not wait long enough: {:?}",
			elapsed
		);
	});
	handle1.join().unwrap();
	handle2.join().unwrap();
}

#[test]
fn test_many_rapid_writer_attempts() {
	let db = Arc::new(temp_db());
	let mut handles = vec![];
	for _ in 0..10 {
		let db_writer = db.clone();
		handles.push(thread::spawn(move || {
			let write_txn = db_writer.begin_write().unwrap();
			thread::sleep(Duration::from_millis(50));
			drop(write_txn);
		}));
	}
	for h in handles {
		h.join().unwrap();
	}
}

#[test]
fn test_readers_during_rapid_writes() {
	let db = Arc::new(temp_db());
	let table = TableDefinition::<&str, u64>::new("snapshots");
	// Ensure the table exists before spawning readers/writers
	{
		let write_txn = db.begin_write().unwrap();
		{
			let mut t = write_txn.open_table(table).unwrap();
			// Optionally insert an initial value
			let _ = t.insert("key", &0);
		}
		write_txn.commit().unwrap();
	}
	// Writer thread: rapidly updates a value
	let db_writer = db.clone();
	let writer_handle = thread::spawn(move || {
		for i in 0..10 {
			let write_txn = db_writer.begin_write().unwrap();
			{
				let mut t = write_txn.open_table(table).unwrap();
				t.insert("key", &i).unwrap();
			}
			write_txn.commit().unwrap();
			thread::sleep(Duration::from_millis(20));
		}
	});
	// Spawn several readers at different times
	let mut reader_handles = vec![];
	for _ in 0..5 {
		let db_reader = db.clone();
		reader_handles.push(thread::spawn(move || {
			let read_txn = db_reader.begin_read().unwrap();
			let t = read_txn.open_table(table).unwrap();
			let val = t.get("key").unwrap();
			// Each reader should see a consistent value (may be None or any committed value)
			let val_value = val.as_ref().map(|v| v.value());
			if let Some(v) = &val {
				let _ = v.value();
			}
			thread::sleep(Duration::from_millis(50));
			// Still see the same value after sleep
			let val2 = t.get("key").unwrap();
			assert_eq!(val_value, val2.as_ref().map(|v| v.value()));
		}));
		thread::sleep(Duration::from_millis(10));
	}
	for h in reader_handles {
		h.join().unwrap();
	}
	writer_handle.join().unwrap();
}
