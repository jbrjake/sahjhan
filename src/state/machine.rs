// src/state/machine.rs

use std::collections::HashMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::config::ProtocolConfig;
use crate::ledger::chain::Ledger;
use crate::ledger::entry::LedgerError;
use crate::gates::evaluator::{evaluate_gate, GateContext};
use super::sets::{MemberStatus, SetStatus};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum StateError {
    #[error("no transition '{command}' available from state '{state}'")]
    NoTransition { command: String, state: String },

    #[error("gate '{gate_type}' blocked transition: {reason}")]
    GateBlocked { gate_type: String, reason: String },

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
}

impl StateMachine {
    /// Create a new `StateMachine`.
    ///
    /// The current state is determined by scanning the ledger for the most
    /// recent `state_transition` event.  If none exists, the config's initial
    /// state is used.
    pub fn new(config: &ProtocolConfig, ledger: Ledger) -> Self {
        let current_state = Self::derive_state_from_ledger(config, &ledger);
        StateMachine {
            config: config.clone(),
            ledger,
            current_state,
        }
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

    /// Attempt to execute a named command from the current state.
    ///
    /// Evaluates all gates on the matching transition; if all pass the
    /// `state_transition` event is appended to the ledger and the current
    /// state is updated.
    pub fn transition(&mut self, command: &str, _args: &[String]) -> Result<(), StateError> {
        // Find a matching transition from the current state.
        let transition = self
            .config
            .transitions
            .iter()
            .find(|t| t.command == command && t.from == self.current_state)
            .ok_or_else(|| StateError::NoTransition {
                command: command.to_string(),
                state: self.current_state.clone(),
            })?
            .clone(); // clone so we release the borrow on self.config

        // Evaluate gates.
        for gate in &transition.gates {
            self.evaluate_gate(gate)?;
        }

        // Record the transition event.
        let mut fields = HashMap::new();
        fields.insert("from".to_string(), self.current_state.clone());
        fields.insert("to".to_string(), transition.to.clone());
        fields.insert("command".to_string(), command.to_string());

        let payload = serialize_fields(&fields)?;
        self.ledger
            .append("state_transition", payload)
            .map_err(StateError::Ledger)?;

        self.current_state = transition.to.clone();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Event recording
    // -----------------------------------------------------------------------

    /// Serialize `fields` as MessagePack and append an entry to the ledger.
    pub fn record_event(
        &mut self,
        event_type: &str,
        fields: HashMap<String, String>,
    ) -> Result<(), StateError> {
        let payload = serialize_fields(&fields)?;
        self.ledger
            .append(event_type, payload)
            .map_err(StateError::Ledger)
    }

    // -----------------------------------------------------------------------
    // Set status
    // -----------------------------------------------------------------------

    /// Return the completion status of the named set.
    pub fn set_status(&self, set_name: &str) -> SetStatus {
        let set_config = self
            .config
            .sets
            .get(set_name)
            .expect("set_status called with unknown set name");

        let completed_members = self.completed_members_for_set(set_name, "set_member_complete", "member");

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

    /// Read the ledger to find the most recent `state_transition` event and
    /// extract the `"to"` field.  Falls back to the config initial state.
    fn derive_state_from_ledger(config: &ProtocolConfig, ledger: &Ledger) -> String {
        let transitions = ledger.events_of_type("state_transition");
        if let Some(last) = transitions.last() {
            if let Ok(fields) = deserialize_fields(&last.payload) {
                if let Some(to) = fields.get("to") {
                    return to.clone();
                }
            }
        }
        config
            .initial_state()
            .unwrap_or("idle")
            .to_string()
    }

    /// Evaluate a single gate using the full gate evaluator.
    fn evaluate_gate(&self, gate: &crate::config::GateConfig) -> Result<(), StateError> {
        let ctx = GateContext {
            ledger: &self.ledger,
            config: &self.config,
            current_state: &self.current_state,
            state_params: HashMap::new(),
            working_dir: PathBuf::from("."),
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

    /// Scan ledger events of `event_type` and collect unique values of
    /// `field_name` where the payload also contains `"set" == set_name`.
    fn completed_members_for_set(
        &self,
        set_name: &str,
        event_type: &str,
        field_name: &str,
    ) -> Vec<String> {
        let mut covered = Vec::new();
        for entry in self.ledger.events_of_type(event_type) {
            if let Ok(fields) = deserialize_fields(&entry.payload) {
                let set_matches = fields
                    .get("set")
                    .map(|v| v.as_str() == set_name)
                    .unwrap_or(false);
                if set_matches {
                    if let Some(member) = fields.get(field_name) {
                        if !covered.contains(member) {
                            covered.push(member.clone());
                        }
                    }
                }
            }
        }
        covered
    }
}

// ---------------------------------------------------------------------------
// MessagePack serialization helpers
// ---------------------------------------------------------------------------

/// Serialize a `HashMap<String, String>` to MessagePack bytes.
fn serialize_fields(fields: &HashMap<String, String>) -> Result<Vec<u8>, StateError> {
    rmp_serde::to_vec(fields).map_err(|e| StateError::Serialization(e.to_string()))
}

/// Deserialize MessagePack bytes to a `HashMap<String, String>`.
fn deserialize_fields(payload: &[u8]) -> Result<HashMap<String, String>, StateError> {
    rmp_serde::from_slice(payload).map_err(|e| StateError::Serialization(e.to_string()))
}
