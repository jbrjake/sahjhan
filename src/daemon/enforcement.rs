// src/daemon/enforcement.rs
//
// The two pieces of enforcement-state logic that are pure functions of their
// inputs: the recursive merge a per-actor write needs, and the version token
// a cross-actor read-modify-write compares against. Both are held here rather
// than inline in `handle_request` so they can be tested without a live daemon
// — the socket handlers in `mod.rs` own the vault lock, these own the meaning.
//
// ## Index
// - merge_patch               — RFC 7386 JSON Merge Patch: recurse where both sides are objects, `null` deletes
// - version_of                — opaque CAS token for a stored blob (`sha256:<hex>` of the stored bytes)

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Apply an RFC 7386 JSON Merge Patch to `target`, in place.
///
/// The rules, and why each one is the one the consumer needs (#49):
///
/// - A patch key whose value is an object and whose stored value is also an
///   object **recurses**. This is the whole point: `{"stall": {"agent-a": 3}}`
///   changes `agent-a`'s entry and leaves every sibling actor alone. The
///   top-level `enforcement_update` op cannot express that — its
///   `Map::extend` replaces `stall` wholesale — so a per-actor writer has to
///   send the entire map and clobbers whatever landed since it read.
/// - A patch key whose value is `null` **deletes** that key. "Absent from the
///   patch" has to keep meaning "unchanged", so deletion needs its own
///   spelling, and RFC 7386 already has one. The cost is that a merge cannot
///   *store* a JSON null; `enforcement_update` still can.
/// - Anything else (scalars, arrays, an object over a non-object) replaces.
///   Arrays are deliberately not merged element-wise: RFC 7386 does not, and
///   a consumer discharging entries from a list wants a replace.
///
/// Recursion is bounded by serde_json's own 128-deep parse limit — both the
/// stored blob and the patch arrived through it — so a hostile patch cannot
/// drive this past the parser's depth.
// [merge-patch]
pub fn merge_patch(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (key, patch_value) in patch {
        match patch_value {
            Value::Null => {
                target.remove(key);
            }
            Value::Object(sub) => {
                // RFC 7386: an object patch over a non-object (or absent)
                // target applies to an empty object, which drops the nested
                // nulls rather than storing them. A stored null would be
                // served back to a consumer that never wrote one.
                let entry = target
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if !entry.is_object() {
                    *entry = Value::Object(Map::new());
                }
                if let Value::Object(existing) = entry {
                    merge_patch(existing, sub);
                }
            }
            other => {
                target.insert(key.clone(), other.clone());
            }
        }
    }
}

/// The version token for a stored enforcement blob: a hash of the bytes as
/// stored.
///
/// Opaque to the caller — compare it, do not parse it. It exists so a
/// read-modify-write that *must* span the whole blob (discharging an entry
/// from whichever actor's bucket holds it) can be made conditional: read
/// returns the token, a mutation carrying `expect_version` is checked against
/// the current one **inside the vault lock**, and a caller that loses the race
/// gets `version_conflict` instead of silently overwriting the winner.
///
/// Computed over the *stored* bytes, not the bytes `enforcement_read` serves.
/// Read overlays `state` from the ledger, and a transition moves the ledger
/// without touching the vault; hashing the served bytes would invalidate every
/// outstanding token on a transition that changed nothing the caller is
/// racing over.
// [version-of]
pub fn version_of(stored: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(stored)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: &str) -> Map<String, Value> {
        match serde_json::from_str(json).unwrap() {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    fn merged(state: &str, patch: &str) -> Value {
        let mut state = obj(state);
        merge_patch(&mut state, &obj(patch));
        Value::Object(state)
    }

    // -- the reported defect ------------------------------------------------

    #[test]
    fn nested_object_merge_leaves_siblings_alone() {
        let out = merged(
            r#"{"stall": {"": 0, "agent-a": 3, "agent-b": 1}}"#,
            r#"{"stall": {"agent-a": 4}}"#,
        );
        assert_eq!(out["stall"]["agent-a"], 4);
        assert_eq!(out["stall"]["agent-b"], 1, "sibling must survive");
        assert_eq!(out["stall"][""], 0, "sibling must survive");
    }

    #[test]
    fn interleaved_actor_writes_both_survive() {
        // The reproduction from #49, with the merge op: agent-a reads a
        // snapshot, agent-b bumps itself, agent-a then writes from the stale
        // snapshot. Under a top-level merge agent-b's bump is lost; under this
        // one agent-a only ever names its own key.
        let mut state = obj(r#"{"stall": {"agent-b": 1}}"#);
        merge_patch(&mut state, &obj(r#"{"stall": {"agent-b": 2}}"#));
        merge_patch(&mut state, &obj(r#"{"stall": {"agent-a": 1}}"#));
        assert_eq!(state["stall"]["agent-b"], 2, "agent-b's bump survives");
        assert_eq!(state["stall"]["agent-a"], 1);
    }

    #[test]
    fn untouched_top_level_keys_are_preserved() {
        let out = merged(
            r#"{"stall": {"a": 1}, "unregistered_commits": {"a": ["abc"]}}"#,
            r#"{"stall": {"a": 2}}"#,
        );
        assert_eq!(out["unregistered_commits"]["a"][0], "abc");
    }

    // -- deletion -----------------------------------------------------------

    #[test]
    fn null_deletes_a_nested_key() {
        let out = merged(
            r#"{"stall": {"agent-a": 3, "agent-b": 1}}"#,
            r#"{"stall": {"agent-a": null}}"#,
        );
        assert!(out["stall"].get("agent-a").is_none());
        assert_eq!(out["stall"]["agent-b"], 1);
    }

    #[test]
    fn null_deletes_a_top_level_key() {
        let out = merged(r#"{"stall": {"a": 1}, "keep": 2}"#, r#"{"stall": null}"#);
        assert!(out.get("stall").is_none());
        assert_eq!(out["keep"], 2);
    }

    #[test]
    fn deleting_an_absent_key_is_a_no_op() {
        let out = merged(r#"{"keep": 1}"#, r#"{"gone": null, "nested": {"x": null}}"#);
        assert_eq!(out["keep"], 1);
        assert!(out.get("gone").is_none());
        // An object patch over an absent key applies to an empty object, so
        // the nested null leaves an empty map rather than a stored null.
        assert_eq!(out["nested"], serde_json::json!({}));
    }

    // -- replacement --------------------------------------------------------

    #[test]
    fn arrays_replace_rather_than_concatenate() {
        let out = merged(
            r#"{"unregistered_commits": {"a": ["abc", "def"]}}"#,
            r#"{"unregistered_commits": {"a": ["def"]}}"#,
        );
        assert_eq!(out["unregistered_commits"]["a"], serde_json::json!(["def"]));
    }

    #[test]
    fn scalar_over_object_replaces_it() {
        let out = merged(r#"{"stall": {"a": 1}}"#, r#"{"stall": 0}"#);
        assert_eq!(out["stall"], 0);
    }

    #[test]
    fn object_over_scalar_replaces_it() {
        let out = merged(r#"{"stall": 0}"#, r#"{"stall": {"a": 1}}"#);
        assert_eq!(out["stall"]["a"], 1);
    }

    #[test]
    fn deep_nesting_recurses_all_the_way_down() {
        let out = merged(
            r#"{"a": {"b": {"c": {"d": 1, "keep": 9}}}}"#,
            r#"{"a": {"b": {"c": {"d": 2}}}}"#,
        );
        assert_eq!(out["a"]["b"]["c"]["d"], 2);
        assert_eq!(out["a"]["b"]["c"]["keep"], 9);
    }

    #[test]
    fn empty_patch_changes_nothing() {
        let out = merged(r#"{"stall": {"a": 1}}"#, r#"{}"#);
        assert_eq!(out, serde_json::json!({"stall": {"a": 1}}));
    }

    // -- version token ------------------------------------------------------

    #[test]
    fn version_is_stable_for_identical_bytes() {
        assert_eq!(version_of(b"{\"a\":1}"), version_of(b"{\"a\":1}"));
    }

    #[test]
    fn version_changes_with_the_bytes() {
        assert_ne!(version_of(b"{\"a\":1}"), version_of(b"{\"a\":2}"));
    }

    #[test]
    fn version_is_a_prefixed_hex_digest() {
        let v = version_of(b"{}");
        assert!(v.starts_with("sha256:"), "got {}", v);
        assert_eq!(v.len(), "sha256:".len() + 64);
    }
}
