use super::*;
use rustls::pki_types::pem::{Error as PemError, PemObject};

pub(crate) fn ensure_leaf_certificate(
    ca_dir: &Path,
    ca_material: &crate::CaMaterial,
    host: &str,
) -> io::Result<(PathBuf, PathBuf)> {
    let leaf_dir = ca_dir.join("leaf");
    let cache_name = leaf_cache_name(host);
    let cert_path = leaf_dir.join(format!("{cache_name}.pem"));
    let key_path = leaf_dir.join(format!("{cache_name}-key.pem"));
    let chain_path = leaf_dir.join(format!("{cache_name}-chain.pem"));
    if cert_path.is_file() && key_path.is_file() && chain_path.is_file() {
        return Ok((chain_path, key_path));
    }

    fs::create_dir_all(&leaf_dir)?;
    let (cert_pem, key_pem, chain_pem) = generate_leaf_certificate(ca_material, host)?;
    fs::write(&cert_path, cert_pem)?;
    fs::write(&chain_path, chain_pem)?;
    write_private_key(&key_path, key_pem.as_bytes())?;
    Ok((chain_path, key_path))
}

pub(crate) fn generate_leaf_certificate(
    ca_material: &crate::CaMaterial,
    host: &str,
) -> io::Result<(String, String, String)> {
    let issued = crate::issue_leaf_certificate(
        ca_material.certificate_pem(),
        ca_material.private_key_pem(),
        host,
    )
    .map_err(io::Error::other)?;
    Ok((
        issued.certificate_pem,
        issued.private_key_pem,
        issued.chain_pem,
    ))
}

pub(crate) fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    let pem = crate::bounded_io::read_file(
        path,
        rsproxy_rules::MAX_RULE_TLS_PEM_BYTES,
        "PEM certificate",
    )?;
    parse_certs(&pem)
}

pub(crate) fn load_certs_from_pem(pem: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    parse_certs(pem.as_bytes())
}

pub(crate) fn load_private_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    let pem = crate::bounded_io::read_file(
        path,
        rsproxy_rules::MAX_RULE_TLS_PEM_BYTES,
        "PEM private key",
    )?;
    parse_private_key(&pem)
}

fn parse_certs(pem: &[u8]) -> io::Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_slice_iter(pem)
        .map(|certificate| certificate.map_err(invalid_pem))
        .collect()
}

fn parse_private_key(pem: &[u8]) -> io::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_slice(pem).map_err(invalid_pem)
}

fn invalid_pem(source: PemError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, InvalidPem { source })
}

#[derive(Debug, thiserror::Error)]
#[error("invalid PEM data: {source}")]
struct InvalidPem {
    #[source]
    source: PemError,
}

fn leaf_cache_name(host: &str) -> String {
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

fn write_private_key(path: &Path, body: &[u8]) -> io::Result<()> {
    fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
