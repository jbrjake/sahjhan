// src/hooks/mod.rs
//
// Hook bridge generation for Claude Code integration.
// Produces Python hook scripts that enforce write protection
// and detect unauthorized modifications to managed paths.

pub mod generate;

pub use generate::{GeneratedHook, HookGenerator};
