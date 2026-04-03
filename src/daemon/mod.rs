// src/daemon/mod.rs
//
// Daemon mode: holds secrets in process memory, serves signing and vault
// operations over a Unix domain socket.
//
// ## Index
// - DaemonServer              -- main server struct (defined in later task)
// - mod platform              -- OS-specific APIs
// - mod vault                 -- in-memory secret store
// - mod protocol              -- wire protocol types
// - mod auth                  -- caller authentication

pub mod platform;
pub mod protocol;
pub mod vault;
