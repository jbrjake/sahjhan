// src/config/vault_policy.rs
//
// Deserialization structs for vault.toml — per-key, state-based access
// control for the daemon's in-memory vault.
//
// The vault is a generic K/V secret store managed by the daemon. A consumer
// may declare that a given vault key is only writable / readable / deletable
// while the ledger's current state is one of a named set. The daemon enforces
// this declaratively: it derives the current state from the active ledger and
// rejects the op otherwise. This keeps state-gating policy in the consumer's
// TOML rather than in imperative hook code.
//
// ## Index
// - VaultPolicyFile   — top-level wrapper ([[policy]] array)
// - VaultPolicy       — one key's access policy; None field = unrestricted for
//                       that op, Some([]) = never permitted, Some([s..]) = only
//                       in those states.

use serde::Deserialize;

/// Wrapper for the full vault.toml file. Optional file; absent = no policies.
#[derive(Debug, Deserialize, Default)]
pub struct VaultPolicyFile {
    #[serde(default, rename = "policy")]
    pub policies: Vec<VaultPolicy>,
}

/// State-based access policy for a single vault key.
///
/// Semantics per op field:
/// - `None` — no constraint; the op is allowed in any state (backward
///   compatible: keys without a policy behave exactly as before).
/// - `Some(vec![])` — the op is never permitted (an explicit lock).
/// - `Some(states)` — the op is permitted only while the current state is one
///   of `states`.
#[derive(Debug, Deserialize, Clone)]
pub struct VaultPolicy {
    pub name: String,
    #[serde(default)]
    pub writable_in_states: Option<Vec<String>>,
    #[serde(default)]
    pub readable_in_states: Option<Vec<String>>,
    #[serde(default)]
    pub deletable_in_states: Option<Vec<String>>,
}

/// Which vault operation an access check applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultAccess {
    Store,
    Read,
    Delete,
}

impl VaultAccess {
    /// Human-readable adjective for diagnostics ("writable"/"readable"/"deletable").
    pub fn adjective(self) -> &'static str {
        match self {
            VaultAccess::Store => "writable",
            VaultAccess::Read => "readable",
            VaultAccess::Delete => "deletable",
        }
    }
}

impl VaultPolicy {
    /// Return the state whitelist for `access`, if any is declared.
    pub fn states_for(&self, access: VaultAccess) -> &Option<Vec<String>> {
        match access {
            VaultAccess::Store => &self.writable_in_states,
            VaultAccess::Read => &self.readable_in_states,
            VaultAccess::Delete => &self.deletable_in_states,
        }
    }

    /// Whether `access` is permitted in `current_state`.
    ///
    /// `None` whitelist → always allowed. `Some(states)` → allowed iff
    /// `current_state` is a member.
    pub fn permits(&self, access: VaultAccess, current_state: &str) -> bool {
        match self.states_for(access) {
            None => true,
            Some(states) => states.iter().any(|s| s == current_state),
        }
    }
}
