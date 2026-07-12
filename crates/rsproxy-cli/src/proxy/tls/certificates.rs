use super::*;

pub(crate) fn ensure_leaf_certificate(ca_dir: &Path, host: &str) -> io::Result<(PathBuf, PathBuf)> {
    let leaf_dir = ca_dir.join("leaf");
    let cache_name = leaf_cache_name(host);
    let cert_path = leaf_dir.join(format!("{cache_name}.pem"));
    let key_path = leaf_dir.join(format!("{cache_name}-key.pem"));
    let chain_path = leaf_dir.join(format!("{cache_name}-chain.pem"));
    if cert_path.is_file() && key_path.is_file() && chain_path.is_file() {
        return Ok((chain_path, key_path));
    }

    fs::create_dir_all(&leaf_dir)?;
    let (cert_pem, key_pem, chain_pem) = generate_leaf_certificate(ca_dir, host)?;
    fs::write(&cert_path, cert_pem)?;
    fs::write(&chain_path, chain_pem)?;
    write_private_key(&key_path, key_pem.as_bytes())?;
    Ok((chain_path, key_path))
}

fn generate_leaf_certificate(ca_dir: &Path, host: &str) -> io::Result<(String, String, String)> {
    let ca_cert_path = ca_dir.join("rsproxy-root-ca.pem");
    let ca_key_path = ca_dir.join("rsproxy-root-ca-key.pem");
    let ca_cert = fs::read_to_string(&ca_cert_path)?;
    let ca_key = fs::read_to_string(&ca_key_path)?;
    let ca_key = KeyPair::from_pem(&ca_key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let issuer = Issuer::from_ca_cert_pem(&ca_cert, ca_key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    let mut params = CertificateParams::new(vec![host.to_string()])
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    params.distinguished_name.push(DnType::CommonName, host);
    params.is_ca = IsCa::NoCa;
    params.use_authority_key_identifier_extension = true;
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);

    let key = KeyPair::generate().map_err(io::Error::other)?;
    let cert = params.signed_by(&key, &issuer).map_err(io::Error::other)?;
    let cert_pem = cert.pem();
    let key_pem = key.serialize_pem();
    let chain_pem = format!("{cert_pem}{ca_cert}");
    Ok((cert_pem, key_pem, chain_pem))
}

pub(crate) fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::certs(&mut reader).collect()
}

pub(crate) fn load_private_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing private key"))
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
