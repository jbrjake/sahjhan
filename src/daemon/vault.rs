// src/daemon/vault.rs
//
// In-memory key-value store for secrets. All values are wrapped in
// Zeroizing<Vec<u8>> so they are securely zeroed on drop.
//
// ## Index
// - Vault                     — in-memory store struct
// - Vault::new                — create empty vault
// - Vault::store              — insert or overwrite an entry
// - Vault::read               — read an entry by name
// - Vault::delete             — remove and zero an entry
// - Vault::list               — list entry names

use std::collections::HashMap;
use zeroize::Zeroizing;

pub struct Vault {
    entries: HashMap<String, Zeroizing<Vec<u8>>>,
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}

impl Vault {
    pub fn new() -> Self {
        Vault {
            entries: HashMap::new(),
        }
    }

    pub fn store(&mut self, name: String, data: Vec<u8>) {
        self.entries.insert(name, Zeroizing::new(data));
    }

    pub fn read(&self, name: &str) -> Option<&[u8]> {
        self.entries.get(name).map(|z| z.as_slice())
    }

    pub fn delete(&mut self, name: &str) {
        self.entries.remove(name);
    }

    pub fn list(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }
}
