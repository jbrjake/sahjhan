use super::entry::LedgerEntry;
use serde::Serialize;

#[derive(Serialize)]
struct GenesisPayload {
    protocol_name: String,
    protocol_version: String,
    format_version: u8,
}

/// Create the genesis (first) entry for a new ledger.
///
/// The genesis entry has seq 0. Its `prev_hash` is a cryptographically random
/// nonce so that two ledgers initialised with the same parameters are still
/// distinguishable.
pub fn create_genesis(protocol_name: &str, protocol_version: &str) -> LedgerEntry {
    let mut nonce = [0u8; 32];
    getrandom::getrandom(&mut nonce).expect("CSPRNG failed");

    let payload = GenesisPayload {
        protocol_name: protocol_name.to_string(),
        protocol_version: protocol_version.to_string(),
        format_version: 1,
    };
    let payload_bytes = rmp_serde::to_vec(&payload).unwrap();

    LedgerEntry::new(0, nonce, "protocol_init".to_string(), payload_bytes)
}
