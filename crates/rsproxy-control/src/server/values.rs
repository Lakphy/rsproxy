use super::ControlState;
use std::fs;

pub(super) fn valid_value_key(key: &str) -> bool {
    rsproxy_rules::valid_value_key(key)
}

pub(super) fn value_keys(state: &ControlState) -> Vec<String> {
    let values_dir = state.options.storage.join("values");
    let mut keys = Vec::new();
    if let Ok(entries) = fs::read_dir(values_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
                keys.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    keys.sort();
    keys
}
