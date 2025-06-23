//! Integration test suite for all redb demonstration modules.
//!
//! This file imports all demo modules from tests/redb_demo/ so they can be run together with:
//!     cargo test --test redb_demo

#[path = "redb_demo/basic.rs"]
mod basic;
#[path = "redb_demo/batch.rs"]
mod batch;
#[path = "redb_demo/multimap.rs"]
mod multimap;
#[path = "redb_demo/mvcc.rs"]
mod mvcc;
#[path = "redb_demo/savepoint.rs"]
mod savepoint;
#[path = "redb_demo/stats.rs"]
mod stats;
#[path = "redb_demo/writer_contention.rs"]
mod writer_contention;
#[path = "redb_demo/zero_copy.rs"]
mod zero_copy;
