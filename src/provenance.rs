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
// The answer is not a field. `fields` is a schema of what a *writer supplies*,
// and every entry in it is a slot someone fills; a provenance value a caller
// could fill in would be worth exactly nothing. So provenance sits where the
// engine's other records of an append already sit — beside `seq`, `ts`, `hash`
// and `prev` on the entry itself. Nothing declares it, nothing can write it,
// and `sahjhan query` exposes it as the column `recorded_by` the same way it
// exposes `ts`.
//
// `Ledger::append` therefore *requires* a `Recorder`. There is no default and
// no inference: a new way to reach the ledger cannot be added without saying
// which one it is, because the code will not compile until it does.
//
// ## Index
// - RECORDED_BY          — the entry key / SQL column carrying it
// - Recorder             — every write path that can append to a ledger
// - [recorder-id]        Recorder::id()  — the value each path records

/// The ledger entry key, and the SQL column, carrying an append's provenance.
///
/// Named once so the entry, `sahjhan query`'s schema, and lint cannot drift
/// about what the column is called.
pub const RECORDED_BY: &str = "recorded_by";

/// A write path that can append to a ledger.
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
    /// The `state_transition` and `gate_attestation` entries a transition
    /// writes about itself. Same guarantee as [`Recorder::Emit`] and the same
    /// reason: the gates ran.
    Transition(String),
    /// The genesis entry, written by `sahjhan init`.
    Init,
    /// `sahjhan ledger import`, which wraps JSONL from somewhere else. It says
    /// only that the row was imported — the engine validated none of it and
    /// knows nothing about where it came from.
    Import(String),
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
    /// The value this write path records in the entry's `recorded_by`.
    pub fn id(&self) -> String {
        match self {
            Recorder::AgentCli => "agent:cli".to_string(),
            Recorder::AgentHookEval => "agent:hook-eval".to_string(),
            Recorder::AuthedCli => "authed:cli".to_string(),
            Recorder::Emit(command) => format!("engine:emit:{}", command),
            Recorder::Transition(command) => format!("engine:transition:{}", command),
            Recorder::Init => "engine:init".to_string(),
            Recorder::Import(source) => format!("import:{}", source),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn every_recorder() -> Vec<Recorder> {
        vec![
            Recorder::AgentCli,
            Recorder::AgentHookEval,
            Recorder::AuthedCli,
            Recorder::Emit("defer_low".to_string()),
            Recorder::Emit("defer_medium".to_string()),
            Recorder::Transition("fix_commit".to_string()),
            Recorder::Init,
            Recorder::Import("legacy.jsonl".to_string()),
            Recorder::TrustedCaller("hooks/courier.py".to_string()),
            Recorder::UnverifiedPeer,
        ]
    }

    #[test]
    fn identities_are_distinct() {
        let ids: Vec<String> = every_recorder().iter().map(|r| r.id()).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "recorder ids collide: {ids:?}");
    }

    #[test]
    fn no_identity_is_empty() {
        // An empty `recorded_by` means "written before this key existed", and
        // hashes as an absent key. A live write path must never produce one.
        for r in every_recorder() {
            assert!(!r.id().is_empty(), "{r:?} must record something");
        }
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
        assert!(!Recorder::Transition("fix_commit".to_string()).is_agent_reachable());
    }
}
