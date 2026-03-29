// src/state/machine.rs
//
// Core state machine: derives state from ledger, executes transitions with gate checks.
//
// ## Index
// - StateError               — NoTransition, GateBlocked, AllCandidatesBlocked, Ledger, Serialization, UnknownSet
// - StateMachine             — owns config + ledger, executes transitions
// - [transition]             transition()              — execute named command (multi-candidate branching with fallthrough)
// - [record-event]           record_event()            — append event to ledger
// - [set-status]             set_status()              — completion status of a named set
// - [build-state-params]     build_state_params()      — derive params from state config + source field
// - [derive-state]           derive_state_from_ledger() — find current state from ledger
// - [evaluate-gate]          evaluate_gate()           — evaluate single gate with context
// - [completed-members]      completed_members_for_set() — scan ledger for completed set members

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use thiserror::Error;

use super::sets::{MemberStatus, SetStatus};
use crate::config::ProtocolConfig;
use crate::gates::evaluator::{evaluate_gate, evaluate_gates, GateContext};
use crate::ledger::chain::Ledger;
use crate::ledger::entry::LedgerError;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum StateError {
    #[error("no transition '{command}' available from state '{state}'")]
    NoTransition { command: String, state: String },

    #[error("gate '{gate_type}' blocked transition: {reason}")]
    GateBlocked { gate_type: String, reason: String },

    #[error("all transition candidates for '{command}' from '{state}' were blocked")]
    AllCandidatesBlocked {
        command: String,
        state: String,
        /// (target_state, gate_type, reason) per failed candidate
        candidates: Vec<(String, String, String)>,
    },

    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("unknown set '{0}'")]
    UnknownSet(String),
}

// ---------------------------------------------------------------------------
// StateMachine
// ---------------------------------------------------------------------------

pub struct StateMachine {
    config: ProtocolConfig,
    ledger: Ledger,
    current_state: String,
    /// Working directory for shell command gates.
    working_dir: PathBuf,
}

impl StateMachine {
    /// Create a new `StateMachine`.
    ///
    /// The current state is determined by scanning the ledger for the most
    /// recent `state_transition` event.  If none exists, the config's initial
    /// state is used.
    ///
    /// `working_dir` defaults to `std::env::current_dir()`.
    pub fn new(config: &ProtocolConfig, ledger: Ledger) -> Self {
        let current_state = Self::derive_state_from_ledger(config, &ledger);
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        StateMachine {
            config: config.clone(),
            ledger,
            current_state,
            working_dir,
        }
    }

    /// Set the working directory for shell command execution.
    pub fn set_working_dir(&mut self, dir: PathBuf) {
        self.working_dir = dir;
    }

    /// Return the current working directory.
    pub fn working_dir(&self) -> &PathBuf {
        &self.working_dir
    }

    /// Return the name of the current state.
    pub fn current_state(&self) -> &str {
        &self.current_state
    }

    /// Immutable access to the underlying ledger.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    // -----------------------------------------------------------------------
    // Transitions
    // -----------------------------------------------------------------------

    // [transition]
    /// Attempt to execute a named command from the current state.
    ///
    /// Collects all transitions matching `command + from` (preserving TOML
    /// declaration order) and tries each in turn.  The first candidate whose
    /// gates all pass wins — its `state_transition` event is appended to the
    /// ledger and the current state is updated.
    ///
    /// If a single candidate is blocked, the backward-compatible `GateBlocked`
    /// error is returned.  If multiple candidates all fail,
    /// `AllCandidatesBlocked` is returned with per-candidate failure details.
    pub fn transition(&mut self, command: &str, args: &[String]) -> Result<(), StateError> {
        // Collect ALL matching candidates (preserving TOML order).
        let candidates: Vec<_> = self
            .config
            .transitions
            .iter()
            .filter(|t| t.command == command && t.from == self.current_state)
            .cloned()
            .collect();

        if candidates.is_empty() {
            return Err(StateError::NoTransition {
                command: command.to_string(),
                state: self.current_state.clone(),
            });
        }

        // Track failures so we can report them if every candidate is blocked.
        // Each entry: (target_state, gate_type, reason)
        let mut failures: Vec<(String, String, String)> = Vec::new();

        for candidate in &candidates {
            // Build state_params from the target state's param definitions.
            let mut state_params = self.build_state_params(&candidate.to);

            // Map CLI args into state_params.
            let mut positional_idx = 0;
            for arg in args {
                if let Some((key, value)) = arg.split_once('=') {
                    state_params.insert(key.to_string(), value.to_string());
                } else if positional_idx < candidate.args.len() {
                    state_params.insert(candidate.args[positional_idx].clone(), arg.clone());
                    positional_idx += 1;
                }
            }

            // Evaluate all gates for this candidate via the evaluator module.
            let ctx = GateContext {
                ledger: &self.ledger,
                config: &self.config,
                current_state: &self.current_state,
                state_params: state_params.clone(),
                working_dir: self.working_dir.clone(),
                event_fields: None,
            };

            let results = evaluate_gates(&candidate.gates, &ctx);
            let first_failure = results.iter().find(|r| !r.passed);

            if let Some(failed) = first_failure {
                // Stash the first failure for this candidate and try the next.
                failures.push((
                    candidate.to.clone(),
                    failed.gate_type.clone(),
                    failed
                        .reason
                        .clone()
                        .unwrap_or_else(|| "gate failed".to_string()),
                ));
                continue;
            }

            // All gates passed — commit this candidate.

            // Reload ledger from disk in case gate commands (e.g. command_succeeds
            // running `sahjhan event ...`) appended entries via a subprocess.
            // Without this, our in-memory seq/prev_hash would be stale. (Issue #3)
            self.ledger.reload().map_err(StateError::Ledger)?;

            // Record the transition event.
            let mut fields = BTreeMap::new();
            fields.insert("from".to_string(), self.current_state.clone());
            fields.insert("to".to_string(), candidate.to.clone());
            fields.insert("command".to_string(), command.to_string());

            self.ledger
                .append("state_transition", fields)
                .map_err(StateError::Ledger)?;

            self.current_state = candidate.to.clone();
            return Ok(());
        }

        // No candidate passed. Choose error style based on candidate count.
        if candidates.len() == 1 {
            // Backward-compatible: single candidate returns GateBlocked.
            let (_, gate_type, reason) = failures.into_iter().next().unwrap();
            Err(StateError::GateBlocked { gate_type, reason })
        } else {
            Err(StateError::AllCandidatesBlocked {
                command: command.to_string(),
                state: self.current_state.clone(),
                candidates: failures,
            })
        }
    }

    // -----------------------------------------------------------------------
    // Event recording
    // -----------------------------------------------------------------------

    // [record-event]
    /// Append an event to the ledger with the given fields.
    pub fn record_event(
        &mut self,
        event_type: &str,
        fields: HashMap<String, String>,
    ) -> Result<(), StateError> {
        let btree_fields: BTreeMap<String, String> = fields.into_iter().collect();
        self.ledger
            .append(event_type, btree_fields)
            .map_err(StateError::Ledger)
    }

    // -----------------------------------------------------------------------
    // Set status
    // -----------------------------------------------------------------------

    // [set-status]
    /// Return the completion status of the named set.
    pub fn set_status(&self, set_name: &str) -> SetStatus {
        let set_config = self
            .config
            .sets
            .get(set_name)
            .expect("set_status called with unknown set name");

        let completed_members =
            self.completed_members_for_set(set_name, "set_member_complete", "member");

        let members: Vec<MemberStatus> = set_config
            .values
            .iter()
            .map(|v| MemberStatus {
                name: v.clone(),
                done: completed_members.contains(v),
            })
            .collect();

        let completed = members.iter().filter(|m| m.done).count();

        SetStatus {
            name: set_name.to_string(),
            total: set_config.values.len(),
            completed,
            members,
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    // [derive-state]
    /// Read the ledger to find the most recent `state_transition` event and
    /// extract the `"to"` field.  Falls back to the config initial state.
    fn derive_state_from_ledger(config: &ProtocolConfig, ledger: &Ledger) -> String {
        let transitions = ledger.events_of_type("state_transition");
        if let Some(last) = transitions.last() {
            if let Some(to) = last.fields.get("to") {
                return to.clone();
            }
        }
        config.initial_state().unwrap_or("idle").to_string()
    }

    // [build-state-params]
    /// Build state_params from a state's param definitions.
    ///
    /// For each `StateParam` in the target state config, the param name is
    /// mapped to a value derived from the set according to `source`:
    /// - `"values"` (default): comma-joined set values
    /// - `"current"`: first incomplete member of the set
    /// - `"last_completed"`: most recently completed member of the set
    fn build_state_params(&self, state_name: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();

        if let Some(state_config) = self.config.states.get(state_name) {
            if let Some(state_params) = &state_config.params {
                for param in state_params {
                    let source = param.source.as_deref().unwrap_or("values");
                    match source {
                        "current" => {
                            if let Some(set_config) = self.config.sets.get(&param.set) {
                                let completed = self.completed_members_for_set(
                                    &param.set,
                                    "set_member_complete",
                                    "member",
                                );
                                if let Some(current) =
                                    set_config.values.iter().find(|v| !completed.contains(v))
                                {
                                    params.insert(param.name.clone(), current.clone());
                                }
                            }
                        }
                        "last_completed" => {
                            let completed = self.completed_members_for_set(
                                &param.set,
                                "set_member_complete",
                                "member",
                            );
                            if let Some(last) = completed.last() {
                                params.insert(param.name.clone(), last.clone());
                            }
                        }
                        _ => {
                            // Default: comma-joined set values
                            if let Some(set_config) = self.config.sets.get(&param.set) {
                                params.insert(param.name.clone(), set_config.values.join(","));
                            }
                        }
                    }
                }
            }
        }

        params
    }

    // [evaluate-gate]
    /// Evaluate a single gate using the full gate evaluator.
    ///
    /// Retained for reference; the `transition()` method now uses
    /// `evaluate_gates()` directly for multi-candidate branching.
    #[allow(dead_code)]
    fn evaluate_gate(
        &self,
        gate: &crate::config::GateConfig,
        state_params: &HashMap<String, String>,
    ) -> Result<(), StateError> {
        let ctx = GateContext {
            ledger: &self.ledger,
            config: &self.config,
            current_state: &self.current_state,
            state_params: state_params.clone(),
            working_dir: self.working_dir.clone(),
            event_fields: None,
        };

        let result = evaluate_gate(gate, &ctx);

        if !result.passed {
            return Err(StateError::GateBlocked {
                gate_type: result.gate_type,
                reason: result.reason.unwrap_or_else(|| "gate failed".to_string()),
            });
        }

        Ok(())
    }

    // [completed-members]
    /// Scan ledger events of `event_type` and collect unique values of
    /// `field_name` where the entry also contains `"set" == set_name`.
    fn completed_members_for_set(
        &self,
        set_name: &str,
        event_type: &str,
        field_name: &str,
    ) -> Vec<String> {
        let mut covered = Vec::new();
        for entry in self.ledger.events_of_type(event_type) {
            let set_matches = entry
                .fields
                .get("set")
                .map(|v| v.as_str() == set_name)
                .unwrap_or(false);
            if set_matches {
                if let Some(member) = entry.fields.get(field_name) {
                    if !covered.contains(member) {
                        covered.push(member.clone());
                    }
                }
            }
        }
        covered
    }
}
