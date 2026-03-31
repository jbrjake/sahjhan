// src/hooks/mod.rs
//
// Hook bridge generation and runtime evaluation for Claude Code integration.
// Produces Python hook scripts that enforce write protection
// and detect unauthorized modifications to managed paths.
// Provides a hook evaluation engine for runtime enforcement.

pub mod eval;
pub mod generate;

pub use eval::{
    evaluate_hooks, AutoRecordResult, HookEvalRequest, HookEvalResult, HookMessage, MonitorWarning,
};
pub use generate::{GeneratedHook, HookGenerator};
