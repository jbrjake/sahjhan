// src/provenance.rs
//
// Who recorded an event, as a field value the caller cannot supply.
//
// `restricted = true` answers "may this event be recorded at all" and answers
// it for the whole event type. That is the wrong granularity for an event that
// must stay agent-writable and still say which writer produced a given row —
// and it is unavailable outright to any event a transition emits, since an emit
// appends directly and would bypass the HMAC proof `restricted` exists to
// require (sahjhan #50).
//
// A field marked `stamped = true` is the finer instrument: the event stays
// writable by everything that could write it before, and the engine fills that
// one field with the identity of the write path it came in on. The value is
// therefore *not* an assertion the caller makes, which is what lets a SQL gate
// filter on it and mean something.
//
// ## Index
// - Recorder             — the write paths that can append a declared event
// - [recorder-id]        Recorder::id()      — the stamp value each path writes
// - STAMP_NAMESPACES     — prefixes reserved for stamp values
// - [is-stamp-value]     is_stamp_value()    — whether a string is in that namespace
// - [stamped-fields]     stamped_fields()    — the fields of an event the engine owns
// - [stamps-for]         stamps_for()        — the (field, value) pairs a recorder writes
// - [reject-supplied]    reject_supplied_stamp() — refuse a caller-supplied stamp

use crate::config::EventConfig;

/// A write path that can append a consumer-declared event.
///
/// The engine can only ever attest to *how* a row arrived, never to who was
/// behind it — so the identities here are named for the path, and the `agent:`
/// ones say plainly that the agent could have produced them. A gate that wants
/// provenance to mean something must exclude those, not enumerate them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorder {
    /// `sahjhan event` — the agent-facing CLI. Anything that can run the binary
    /// can record any declared, non-restricted event this way, including a hook
    /// script that shells out to it. Nothing stronger can honestly be claimed.
    AgentCli,
    /// A hook rule's `auto_record`, appended by `sahjhan hook eval`. The harness
    /// normally invokes it, but so can the agent, with the same arguments — so
    /// this is a sibling of [`Recorder::AgentCli`], not a stronger identity.
    AgentHookEval,
    /// `sahjhan authed-event` — an HMAC proof the daemon verified. Proves the
    /// caller could reach the daemon and get a signature, not which process it is.
    AuthedCli,
    /// A transition's `emits`, carrying the transition command that produced it.
    /// Unforgeable by a caller in the sense that matters: the only way to write
    /// it is to take the transition, gates and all.
    Emit(String),
    /// The daemon's `record_event`, from a peer whose script canonicalized under
    /// the config dir and matched its `trusted-callers.toml` hash. Carries the
    /// manifest-relative script path — the one identity here tied to *content*
    /// rather than to a command line.
    TrustedCaller(String),
    /// The daemon's `record_event` with no `trusted-callers.toml` present, so
    /// caller auth was never configured and every connection is allowed. The
    /// daemon cannot name its peer, and says so rather than implying it can.
    UnverifiedPeer,
}

impl Recorder {
    // [recorder-id]
    /// The value this write path stamps into a `stamped = true` field.
    pub fn id(&self) -> String {
        match self {
            Recorder::AgentCli => "agent:cli".to_string(),
            Recorder::AgentHookEval => "agent:hook-eval".to_string(),
            Recorder::AuthedCli => "authed:cli".to_string(),
            Recorder::Emit(command) => format!("engine:emit:{}", command),
            Recorder::TrustedCaller(path) => format!("hook:{}", path),
            Recorder::UnverifiedPeer => "daemon:unverified".to_string(),
        }
    }

    /// Whether this identity is one the agent can assume at will.
    ///
    /// Read by lint L8: a gate that *requires* one of these is filtering on
    /// something the party it means to exclude can produce on demand.
    pub fn is_agent_reachable(&self) -> bool {
        matches!(self, Recorder::AgentCli | Recorder::AgentHookEval)
    }
}

/// The prefixes a stamp value can start with.
///
/// Reserved: the engine writes these and lint reads them back out of SQL
/// predicates, so a consumer field value that begins with one will be read as a
/// provenance filter. Nothing enforces the reservation at record time — an
/// ordinary field may hold whatever it likes — but a predicate comparing
/// against one of these is treated as naming a writer.
///
/// Each prefix stops one segment short of the values under it (`engine:`, not
/// `engine:emit:`) so that a *misspelled* value is still recognized as one. A
/// namespace that only matched well-formed values would be silent on exactly
/// the case lint exists to catch — `engine:emits:defer_low`, one letter off,
/// filtering on a writer that does not exist.
pub const STAMP_NAMESPACES: &[&str] = &["agent:", "authed:", "engine:", "hook:", "daemon:"];

// [is-stamp-value]
/// Whether `value` sits in the reserved stamp namespace.
pub fn is_stamp_value(value: &str) -> bool {
    STAMP_NAMESPACES.iter().any(|p| value.starts_with(p))
}

// [stamped-fields]
/// The fields of `event` whose values the engine owns.
pub fn stamped_fields(event: &EventConfig) -> impl Iterator<Item = &str> {
    event
        .fields
        .iter()
        .filter(|f| f.stamped)
        .map(|f| f.name.as_str())
}

// [stamps-for]
/// The `(field, value)` pairs `recorder` writes onto an event of this type.
///
/// Empty for an undeclared event type and for one that stamps nothing, so every
/// write path can call this unconditionally.
pub fn stamps_for(event: Option<&EventConfig>, recorder: &Recorder) -> Vec<(String, String)> {
    let Some(event) = event else {
        return Vec::new();
    };
    let id = recorder.id();
    stamped_fields(event)
        .map(|name| (name.to_string(), id.clone()))
        .collect()
}

// [reject-supplied]
/// Refuse a caller that supplied a stamped field itself.
///
/// Overwriting it silently would be the worse failure: a config that forgot to
/// stamp, and one whose stamp is being forged, would then read identically at
/// the gate. The refusal makes the attempt visible at the moment it is made.
pub fn reject_supplied_stamp<'a>(
    event: Option<&EventConfig>,
    event_type: &str,
    supplied: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let Some(event) = event else {
        return Ok(());
    };
    let stamped: Vec<&str> = stamped_fields(event).collect();
    if stamped.is_empty() {
        return Ok(());
    }
    for key in supplied {
        if stamped.contains(&key) {
            return Err(format!(
                "error: field '{}' of event '{}' is stamped — the engine records \
                 who wrote the event, so it cannot be supplied by the caller",
                key, event_type
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EventFieldConfig;

    fn field(name: &str, stamped: bool) -> EventFieldConfig {
        EventFieldConfig {
            name: name.to_string(),
            field_type: "string".to_string(),
            pattern: None,
            values: None,
            optional: false,
            stamped,
        }
    }

    fn event(fields: Vec<EventFieldConfig>) -> EventConfig {
        EventConfig {
            description: "test".to_string(),
            restricted: None,
            producers: vec![],
            attestation: None,
            fields,
        }
    }

    #[test]
    fn every_identity_is_in_the_reserved_namespace() {
        for recorder in [
            Recorder::AgentCli,
            Recorder::AgentHookEval,
            Recorder::AuthedCli,
            Recorder::Emit("defer_low".to_string()),
            Recorder::TrustedCaller("hooks/courier.py".to_string()),
            Recorder::UnverifiedPeer,
        ] {
            let id = recorder.id();
            assert!(
                is_stamp_value(&id),
                "{id} must be recognizable as a stamp value"
            );
        }
    }

    #[test]
    fn identities_are_distinct() {
        let ids = [
            Recorder::AgentCli.id(),
            Recorder::AgentHookEval.id(),
            Recorder::AuthedCli.id(),
            Recorder::Emit("defer_low".to_string()).id(),
            Recorder::Emit("defer_medium".to_string()).id(),
            Recorder::TrustedCaller("hooks/courier.py".to_string()).id(),
            Recorder::UnverifiedPeer.id(),
        ];
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "stamp ids collide: {ids:?}");
    }

    #[test]
    fn the_courier_and_the_agent_are_told_apart() {
        // The whole point of #50: two writers of the same event type, on the
        // same row shape, distinguishable in SQL.
        assert_ne!(
            Recorder::TrustedCaller("enforcement/hooks/theoretical_courier.py".to_string()).id(),
            Recorder::AgentCli.id()
        );
        assert!(Recorder::AgentCli.is_agent_reachable());
        assert!(Recorder::AgentHookEval.is_agent_reachable());
        assert!(!Recorder::TrustedCaller("x.py".to_string()).is_agent_reachable());
        assert!(!Recorder::Emit("defer_low".to_string()).is_agent_reachable());
    }

    #[test]
    fn only_stamped_fields_are_stamped() {
        let ev = event(vec![
            field("id", false),
            field("recorded_by", true),
            field("reason", false),
        ]);
        let stamps = stamps_for(Some(&ev), &Recorder::AgentCli);
        assert_eq!(
            stamps,
            vec![("recorded_by".to_string(), "agent:cli".to_string())]
        );
    }

    #[test]
    fn an_undeclared_event_stamps_nothing() {
        assert!(stamps_for(None, &Recorder::AgentCli).is_empty());
    }

    #[test]
    fn a_supplied_stamp_is_refused() {
        let ev = event(vec![field("id", false), field("recorded_by", true)]);
        let err = reject_supplied_stamp(
            Some(&ev),
            "finding_deferred",
            ["id", "recorded_by"].into_iter(),
        )
        .unwrap_err();
        assert!(err.contains("recorded_by"), "got: {err}");
        assert!(err.contains("stamped"), "got: {err}");

        reject_supplied_stamp(Some(&ev), "finding_deferred", ["id"].into_iter()).unwrap();
    }
}
