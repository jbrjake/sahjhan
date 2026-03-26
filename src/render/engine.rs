// src/render/engine.rs
//
// Tera template rendering engine that generates read-only markdown views
// from ledger state. The agent never writes STATUS.md or other rendered
// files directly — Sahjhan renders them from the event log.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use serde_json::json;
use tera::Tera;

use crate::config::ProtocolConfig;
use crate::ledger::chain::Ledger;
use crate::manifest::tracker::Manifest;

/// A single event formatted for template rendering.
#[derive(Debug, Clone, Serialize)]
pub struct EventSummary {
    pub seq: u64,
    pub event_type: String,
    pub timestamp: String,
    pub fields: HashMap<String, String>,
}

/// Status of a single set member for template rendering.
#[derive(Debug, Clone, Serialize)]
pub struct MemberSummary {
    pub name: String,
    pub done: bool,
}

/// Aggregated status of a set for template rendering.
#[derive(Debug, Clone, Serialize)]
pub struct SetSummary {
    pub completed: usize,
    pub total: usize,
    pub members: Vec<MemberSummary>,
}

/// Template rendering engine powered by Tera.
pub struct RenderEngine {
    tera: Tera,
    config: ProtocolConfig,
}

impl RenderEngine {
    /// Create a new `RenderEngine` by loading templates from the config directory.
    ///
    /// Templates are resolved relative to `config_dir` (the directory containing
    /// protocol.toml, renders.toml, etc.).
    pub fn new(config: &ProtocolConfig, config_dir: &Path) -> Result<Self, String> {
        let mut tera = Tera::default();

        for render_cfg in &config.renders {
            let template_path = config_dir.join(&render_cfg.template);
            let template_src = std::fs::read_to_string(&template_path).map_err(|e| {
                format!("cannot read template '{}': {}", template_path.display(), e)
            })?;
            tera.add_raw_template(&render_cfg.template, &template_src)
                .map_err(|e| format!("cannot parse template '{}': {}", render_cfg.template, e))?;
        }

        Ok(RenderEngine {
            tera,
            config: config.clone(),
        })
    }

    /// Render all configured templates and write them to `render_dir`.
    ///
    /// Each rendered file is tracked in the manifest with `ledger_seq`.
    /// Returns a list of target file paths that were rendered.
    pub fn render_all(
        &self,
        ledger: &Ledger,
        render_dir: &Path,
        manifest: &mut Manifest,
        ledger_seq: u64,
    ) -> Result<Vec<String>, String> {
        let ctx = self.build_context(ledger)?;
        let mut rendered = Vec::new();

        for render_cfg in &self.config.renders {
            let output = self
                .tera
                .render(&render_cfg.template, &ctx)
                .map_err(|e| format!("render error for '{}': {}", render_cfg.template, e))?;

            let target_path = render_dir.join(&render_cfg.target);

            // Ensure parent directory exists
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create directory {}: {}", parent.display(), e))?;
            }

            std::fs::write(&target_path, &output).map_err(|e| {
                format!(
                    "cannot write rendered file '{}': {}",
                    target_path.display(),
                    e
                )
            })?;

            // Track in manifest
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let rel = target_path
                .strip_prefix(&cwd)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| target_path.to_string_lossy().to_string());
            manifest
                .track(&rel, &target_path, "render", ledger_seq)
                .map_err(|e| format!("cannot track rendered file: {}", e))?;

            rendered.push(render_cfg.target.clone());
        }

        Ok(rendered)
    }

    /// Render only templates that match a specific trigger type.
    ///
    /// - `trigger` is `"on_transition"` or `"on_event"`.
    /// - For `"on_event"` triggers, `event_type` is checked against the
    ///   render config's `event_types` list.
    pub fn render_triggered(
        &self,
        trigger: &str,
        event_type: Option<&str>,
        ledger: &Ledger,
        render_dir: &Path,
        manifest: &mut Manifest,
        ledger_seq: u64,
    ) -> Result<Vec<String>, String> {
        let ctx = self.build_context(ledger)?;
        let mut rendered = Vec::new();

        for render_cfg in &self.config.renders {
            if render_cfg.trigger != trigger {
                continue;
            }

            // For on_event triggers, check if the event type matches
            if trigger == "on_event" {
                if let Some(et) = event_type {
                    if let Some(allowed) = &render_cfg.event_types {
                        if !allowed.iter().any(|a| a == et) {
                            continue;
                        }
                    }
                } else {
                    // No event type provided but trigger requires it
                    continue;
                }
            }

            let output = self
                .tera
                .render(&render_cfg.template, &ctx)
                .map_err(|e| format!("render error for '{}': {}", render_cfg.template, e))?;

            let target_path = render_dir.join(&render_cfg.target);

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create directory {}: {}", parent.display(), e))?;
            }

            std::fs::write(&target_path, &output).map_err(|e| {
                format!(
                    "cannot write rendered file '{}': {}",
                    target_path.display(),
                    e
                )
            })?;

            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let rel = target_path
                .strip_prefix(&cwd)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| target_path.to_string_lossy().to_string());
            manifest
                .track(&rel, &target_path, "render", ledger_seq)
                .map_err(|e| format!("cannot track rendered file: {}", e))?;

            rendered.push(render_cfg.target.clone());
        }

        Ok(rendered)
    }

    /// Build the Tera context from ledger state and config.
    fn build_context(&self, ledger: &Ledger) -> Result<tera::Context, String> {
        let current_state = derive_current_state(&self.config, ledger);

        let state_label = self
            .config
            .states
            .get(&current_state)
            .map(|s| s.label.clone())
            .unwrap_or_else(|| current_state.clone());

        let mut ctx = tera::Context::new();

        ctx.insert(
            "protocol",
            &json!({
                "name": self.config.protocol.name,
                "version": self.config.protocol.version,
                "description": self.config.protocol.description,
            }),
        );

        ctx.insert(
            "state",
            &json!({
                "name": current_state,
                "label": state_label,
            }),
        );

        // Build events list
        let events: Vec<EventSummary> = ledger
            .entries()
            .iter()
            .map(|entry| {
                let timestamp = chrono::DateTime::from_timestamp_millis(entry.timestamp)
                    .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                    .unwrap_or_else(|| format!("{}ms", entry.timestamp));

                let fields: HashMap<String, String> = if !entry.payload.is_empty() {
                    rmp_serde::from_slice(&entry.payload).unwrap_or_default()
                } else {
                    HashMap::new()
                };

                EventSummary {
                    seq: entry.seq,
                    event_type: entry.event_type.clone(),
                    timestamp,
                    fields,
                }
            })
            .collect();
        ctx.insert("events", &events);

        // Build sets status
        let mut sets: HashMap<String, SetSummary> = HashMap::new();
        for (set_name, set_config) in &self.config.sets {
            let completed_members = completed_members_for_set(ledger, set_name);
            let members: Vec<MemberSummary> = set_config
                .values
                .iter()
                .map(|v| MemberSummary {
                    name: v.clone(),
                    done: completed_members.contains(v),
                })
                .collect();
            let completed = members.iter().filter(|m| m.done).count();

            sets.insert(
                set_name.clone(),
                SetSummary {
                    completed,
                    total: set_config.values.len(),
                    members,
                },
            );
        }
        ctx.insert("sets", &sets);

        ctx.insert("ledger_len", &ledger.len());

        let violations = ledger.events_of_type("protocol_violation").len();
        ctx.insert("violations", &violations);

        Ok(ctx)
    }
}

// ---------------------------------------------------------------------------
// Helpers — derive state and set status directly from ledger without
// requiring ownership (StateMachine::new takes Ledger by value).
// ---------------------------------------------------------------------------

/// Derive current state by scanning ledger for the most recent state_transition.
fn derive_current_state(config: &ProtocolConfig, ledger: &Ledger) -> String {
    let transitions = ledger.events_of_type("state_transition");
    if let Some(last) = transitions.last() {
        if let Ok(fields) = deserialize_fields(&last.payload) {
            if let Some(to) = fields.get("to") {
                return to.clone();
            }
        }
    }
    config.initial_state().unwrap_or("idle").to_string()
}

/// Find all completed members for a given set by scanning set_member_complete events.
fn completed_members_for_set(ledger: &Ledger, set_name: &str) -> Vec<String> {
    let mut covered = Vec::new();
    for entry in ledger.events_of_type("set_member_complete") {
        if let Ok(fields) = deserialize_fields(&entry.payload) {
            let set_matches = fields
                .get("set")
                .map(|v| v.as_str() == set_name)
                .unwrap_or(false);
            if set_matches {
                if let Some(member) = fields.get("member") {
                    if !covered.contains(member) {
                        covered.push(member.clone());
                    }
                }
            }
        }
    }
    covered
}

/// Deserialize MessagePack bytes to a HashMap<String, String>.
fn deserialize_fields(payload: &[u8]) -> Result<HashMap<String, String>, String> {
    rmp_serde::from_slice(payload).map_err(|e| e.to_string())
}
