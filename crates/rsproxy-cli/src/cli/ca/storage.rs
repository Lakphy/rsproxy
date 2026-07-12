use super::*;

pub(in crate::cli) fn ca_cert_info(ca_dir: &Path) -> Result<(PathBuf, String, String), String> {
    let cert_path = ca_dir.join("rsproxy-root-ca.pem");
    let cert =
        fs::read_to_string(&cert_path).map_err(|e| format!("read {}: {e}", cert_path.display()))?;
    let fingerprint = cert_fingerprint(&cert)
        .ok_or_else(|| format!("invalid certificate {}", cert_path.display()))?;
    Ok((cert_path, cert, fingerprint))
}

pub(in crate::cli) fn leaf_cache_name(host: &str) -> String {
    host.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub(in crate::cli) fn leaf_cache_count(ca_dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(ca_dir.join("leaf")) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_type().map(|ty| ty.is_file()).unwrap_or(false)
                && entry.file_name().to_str().is_some_and(|name| {
                    name.ends_with(".pem")
                        && !name.ends_with("-key.pem")
                        && !name.ends_with("-chain.pem")
                })
        })
        .count()
}

pub(in crate::cli) fn write_private_key(path: &Path, body: &[u8]) -> Result<(), String> {
    fs::write(path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
