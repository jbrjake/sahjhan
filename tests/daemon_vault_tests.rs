use sahjhan::daemon::vault::Vault;

#[test]
fn test_vault_stores_reserved_names_at_data_level() {
    let mut vault = Vault::new();
    vault.store("_enforcement".to_string(), b"data".to_vec());
    assert_eq!(vault.read("_enforcement").unwrap(), b"data");
}

#[test]
fn test_vault_store_and_read() {
    let mut vault = Vault::new();
    vault.store("secret".to_string(), b"hello world".to_vec());
    let data = vault.read("secret").unwrap();
    assert_eq!(data, b"hello world");
}

#[test]
fn test_vault_read_not_found() {
    let vault = Vault::new();
    assert!(vault.read("nonexistent").is_none());
}

#[test]
fn test_vault_overwrite() {
    let mut vault = Vault::new();
    vault.store("key".to_string(), b"first".to_vec());
    vault.store("key".to_string(), b"second".to_vec());
    assert_eq!(vault.read("key").unwrap(), b"second");
}

#[test]
fn test_vault_delete() {
    let mut vault = Vault::new();
    vault.store("key".to_string(), b"data".to_vec());
    vault.delete("key");
    assert!(vault.read("key").is_none());
}

#[test]
fn test_vault_delete_nonexistent_is_noop() {
    let mut vault = Vault::new();
    vault.delete("nonexistent");
}

#[test]
fn test_vault_list() {
    let mut vault = Vault::new();
    vault.store("b-key".to_string(), b"data".to_vec());
    vault.store("a-key".to_string(), b"data".to_vec());
    let mut names = vault.list();
    names.sort();
    assert_eq!(names, vec!["a-key", "b-key"]);
}

#[test]
fn test_vault_list_empty() {
    let vault = Vault::new();
    assert!(vault.list().is_empty());
}
