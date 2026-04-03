// src/gates/ledger.rs
//
// ## Index
// - [eval-ledger-has-event]        eval_ledger_has_event()        — pass if ledger contains N+ events of a type (optional max_count ceiling)
// - [eval-ledger-has-event-since]  eval_ledger_has_event_since()  — pass if event exists since reference point (last_transition or custom event type)
// - [eval-ledger-lacks-event]      eval_ledger_lacks_event()      — pass if ledger contains NO matching events (negation of ledger_has_event)
// - [eval-set-covered]             eval_set_covered()             — pass if all set members appear in ledger
// - [eval-min-elapsed]             eval_min_elapsed()             — pass if enough time has elapsed since last event
// - [eval-no-violations]           eval_no_violations()           — pass if no unresolved protocol_violation events
// - [eval-field-not-empty]         eval_field_not_empty()         — pass if named event field is non-empty

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::GateConfig;

use super::evaluator::{GateContext, GateResult};
use super::types::entry_matches_filter;

// [eval-ledger-has-event]
pub(super) fn eval_ledger_has_event(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let min_count = gate
        .params
        .get("min_count")
        .and_then(|v| v.as_integer())
        .map(|n| n as u32)
        .unwrap_or(1);

    let max_count = gate
        .params
        .get("max_count")
        .and_then(|v| v.as_integer())
        .map(|n| n as u32);

    // Optional filter map: each key/value must match the deserialized payload.
    let filter: HashMap<String, String> = gate
        .params
        .get("filter")
        .and_then(|v| v.as_table())
        .map(|tbl| {
            tbl.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let matching = ctx
        .ledger
        .events_of_type(event)
        .into_iter()
        .filter(|e| entry_matches_filter(e, &filter))
        .count();

    let meets_min = matching >= min_count as usize;
    let under_max = max_count.map(|m| matching < m as usize).unwrap_or(true);
    let passed = meets_min && under_max;

    let description = match max_count {
        Some(max) => format!(
            "ledger has >= {} and < {} '{}' event(s)",
            min_count, max, event
        ),
        None => format!("ledger has >= {} '{}' event(s)", min_count, event),
    };

    let reason = if passed {
        None
    } else if !meets_min {
        Some(format!(
            "found {} '{}' event(s), need >= {}",
            matching, event, min_count
        ))
    } else {
        // !under_max
        Some(format!(
            "found {} '{}' event(s), budget exhausted (max {})",
            matching,
            event,
            max_count.unwrap()
        ))
    };

    GateResult {
        passed,
        evaluable: true,
        gate_type: "ledger_has_event".to_string(),
        description,
        reason,
        intent: None,
        attestation: None,
    }
}

// [eval-ledger-has-event-since]
pub(super) fn eval_ledger_has_event_since(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let since = gate
        .params
        .get("since")
        .and_then(|v| v.as_str())
        .unwrap_or("last_transition");

    let threshold_seq = if since == "last_transition" {
        ctx.ledger
            .events_of_type("state_transition")
            .last()
            .map(|e| e.seq)
            .unwrap_or(0)
    } else {
        let since_event_seq = ctx
            .ledger
            .entries()
            .iter()
            .rev()
            .find(|e| e.event_type == since)
            .map(|e| e.seq);
        match since_event_seq {
            Some(seq) => seq,
            None => ctx
                .ledger
                .events_of_type("state_transition")
                .last()
                .map(|e| e.seq)
                .unwrap_or(0),
        }
    };

    let found = ctx
        .ledger
        .entries()
        .iter()
        .any(|e| e.event_type == event && e.seq > threshold_seq);

    let since_desc = if since == "last_transition" {
        "last state_transition".to_string()
    } else {
        format!("last '{}' event", since)
    };

    GateResult {
        passed: found,
        evaluable: true,
        gate_type: "ledger_has_event_since".to_string(),
        description: format!("'{}' event exists since {}", event, since_desc),
        reason: if found {
            None
        } else {
            Some(format!("no '{}' event found after {}", event, since_desc))
        },
        intent: None,
        attestation: None,
    }
}

// [eval-ledger-lacks-event]
pub(super) fn eval_ledger_lacks_event(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let filter: HashMap<String, String> = gate
        .params
        .get("filter")
        .and_then(|v| v.as_table())
        .map(|tbl| {
            tbl.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let matching = ctx
        .ledger
        .events_of_type(event)
        .into_iter()
        .filter(|e| entry_matches_filter(e, &filter))
        .count();

    let passed = matching == 0;

    GateResult {
        passed,
        evaluable: true,
        gate_type: "ledger_lacks_event".to_string(),
        description: format!("ledger has no '{}' events", event),
        reason: if passed {
            None
        } else {
            Some(format!(
                "found {} '{}' event(s), expected none",
                matching, event
            ))
        },
        intent: None,
        attestation: None,
    }
}

// [eval-set-covered]
pub(super) fn eval_set_covered(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let set_name = match gate.params.get("set").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "set_covered".to_string(),
                description: "set is fully covered".to_string(),
                reason: Some("gate missing 'set' param".to_string()),
                intent: None,
                attestation: None,
            }
        }
    };

    let event_name = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("set_member_complete");

    let field_name = gate
        .params
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("member");

    let set_config = match ctx.config.sets.get(set_name) {
        Some(s) => s,
        None => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "set_covered".to_string(),
                description: format!("set '{}' is fully covered", set_name),
                reason: Some(format!("unknown set '{}'", set_name)),
                intent: None,
                attestation: None,
            }
        }
    };

    // Collect the unique values of `field_name` from entries where
    // `"set" == set_name`.  Use HashSet for O(1) membership checks.
    let mut covered: HashSet<String> = HashSet::new();
    for entry in ctx.ledger.events_of_type(event_name) {
        let set_matches = entry
            .fields
            .get("set")
            .map(|v| v.as_str() == set_name)
            .unwrap_or(false);
        if set_matches {
            if let Some(member) = entry.fields.get(field_name) {
                covered.insert(member.clone());
            }
        }
    }

    let missing: Vec<&str> = set_config
        .values
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .map(|v| v.as_str())
        .collect();

    let passed = missing.is_empty();

    GateResult {
        passed,
        evaluable: true,
        gate_type: "set_covered".to_string(),
        description: format!("set '{}' is fully covered", set_name),
        reason: if passed {
            None
        } else {
            Some(format!(
                "set '{}' not fully covered; missing: {}",
                set_name,
                missing.join(", ")
            ))
        },
        intent: None,
        attestation: None,
    }
}

// [eval-min-elapsed]
pub(super) fn eval_min_elapsed(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let seconds = gate
        .params
        .get("seconds")
        .and_then(|v| v.as_integer())
        .map(|s| s as u64)
        .unwrap_or(0);

    // Find the most recent matching event and parse its ISO 8601 timestamp.
    let last_ts_ms = ctx.ledger.events_of_type(event).last().and_then(|e| {
        chrono::DateTime::parse_from_rfc3339(&e.ts)
            .ok()
            .map(|dt| dt.timestamp_millis())
    });

    let description = format!(
        "at least {} second(s) since last '{}' event",
        seconds, event
    );

    match last_ts_ms {
        None => {
            // No event found — consider the elapsed time infinite.
            GateResult {
                passed: true,
                evaluable: true,
                gate_type: "min_elapsed".to_string(),
                description,
                reason: None,
                intent: None,
                attestation: None,
            }
        }
        Some(ts_ms) => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as i64;

            let elapsed_ms = now_ms.saturating_sub(ts_ms);
            let required_ms = (seconds as i64) * 1000;
            let passed = elapsed_ms >= required_ms;

            GateResult {
                passed,
                evaluable: true,
                gate_type: "min_elapsed".to_string(),
                description,
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "only {}ms elapsed since last '{}' event, need {}ms",
                        elapsed_ms, event, required_ms
                    ))
                },
                intent: None,
                attestation: None,
            }
        }
    }
}

// [eval-no-violations]
pub(super) fn eval_no_violations(_gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let violations = ctx.ledger.events_of_type("protocol_violation").len();
    let resolved = ctx.ledger.events_of_type("violation_resolved").len();
    let unresolved = violations.saturating_sub(resolved);
    let passed = unresolved == 0;

    GateResult {
        passed,
        evaluable: true,
        gate_type: "no_violations".to_string(),
        description: "no unresolved protocol_violation events".to_string(),
        reason: if passed {
            None
        } else {
            Some(format!(
                "found {} unresolved protocol_violation event(s) ({} total, {} resolved)",
                unresolved, violations, resolved
            ))
        },
        intent: None,
        attestation: None,
    }
}

// [eval-field-not-empty]
pub(super) fn eval_field_not_empty(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let field = gate
        .params
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let description = format!("field '{}' is non-empty", field);

    let value = ctx
        .event_fields
        .and_then(|fields| fields.get(field))
        .map(|s| s.as_str());

    match value {
        None => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: Some(format!("field '{}' not present in event payload", field)),
            intent: None,
            attestation: None,
        },
        Some("") => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: Some(format!("field '{}' is empty", field)),
            intent: None,
            attestation: None,
        },
        Some(_) => GateResult {
            passed: true,
            evaluable: true,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: None,
            intent: None,
            attestation: None,
        },
    }
}
