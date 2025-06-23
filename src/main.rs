use redb::Database;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

fn main() {
    let db = Arc::new(
        Database::create("mydb_writer_contention.redb").expect("Failed to create database"),
    );
    let barrier = Arc::new(Barrier::new(2));

    // Writer 1: acquires the write transaction and holds it
    let db_writer1 = db.clone();
    let barrier_writer1 = barrier.clone();
    let handle1 = thread::spawn(move || {
        let write_txn = db_writer1.begin_write().unwrap();
        println!("[Writer 1] Acquired write transaction, holding lock...");
        barrier_writer1.wait(); // Signal Writer 2 to start
        thread::sleep(Duration::from_secs(3)); // Hold the lock for a while
        drop(write_txn); // Release the lock
        println!("[Writer 1] Released write transaction");
    });

    // Writer 2: waits for Writer 1 to acquire the lock, then tries to acquire it
    let db_writer2 = db.clone();
    let barrier_writer2 = barrier.clone();
    let handle2 = thread::spawn(move || {
        barrier_writer2.wait(); // Wait for Writer 1 to acquire lock
        println!("[Writer 2] Attempting to acquire write transaction...");
        match db_writer2.begin_write() {
            Ok(_txn) => {
                println!("[Writer 2] Acquired write transaction (after Writer 1 released it)")
            }
            Err(e) => println!("[Writer 2] Failed to acquire write transaction: {}", e),
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}
