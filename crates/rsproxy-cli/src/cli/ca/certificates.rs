use super::*;
use base64::{Engine as _, engine::general_purpose};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
use sha2::{Digest, Sha256};

pub(crate) fn generate_root_ca(common_name: &str) -> Result<(String, String), String> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "rsproxy");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::CrlSign);
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);

    let key = KeyPair::generate().map_err(|e| e.to_string())?;
    let cert = params.self_signed(&key).map_err(|e| e.to_string())?;
    Ok((cert.pem(), key.serialize_pem()))
}

pub(crate) fn generate_leaf_cert(
    ca_dir: &Path,
    host: &str,
) -> Result<(String, String, String), String> {
    let ca_cert_path = ca_dir.join("rsproxy-root-ca.pem");
    let ca_key_path = ca_dir.join("rsproxy-root-ca-key.pem");
    let ca_cert = fs::read_to_string(&ca_cert_path)
        .map_err(|e| format!("read {}: {e}", ca_cert_path.display()))?;
    let ca_key = fs::read_to_string(&ca_key_path)
        .map_err(|e| format!("read {}: {e}", ca_key_path.display()))?;
    let ca_key = KeyPair::from_pem(&ca_key).map_err(|e| e.to_string())?;
    let issuer = Issuer::from_ca_cert_pem(&ca_cert, ca_key).map_err(|e| e.to_string())?;

    let mut params = CertificateParams::new(vec![host.to_string()]).map_err(|e| e.to_string())?;
    params.distinguished_name.push(DnType::CommonName, host);
    params.is_ca = IsCa::NoCa;
    params.use_authority_key_identifier_extension = true;
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);

    let key = KeyPair::generate().map_err(|e| e.to_string())?;
    let cert = params.signed_by(&key, &issuer).map_err(|e| e.to_string())?;
    let cert_pem = cert.pem();
    let key_pem = key.serialize_pem();
    let chain_pem = format!("{cert_pem}{ca_cert}");
    Ok((cert_pem, key_pem, chain_pem))
}

pub(in crate::cli) fn validate_leaf_host(host: &str) -> Result<(), String> {
    if host.trim().is_empty() || host.contains('/') || host.chars().any(char::is_whitespace) {
        return Err(format!("invalid certificate host `{host}`"));
    }
    Ok(())
}

pub(in crate::cli) fn cert_fingerprint(cert_pem: &str) -> Option<String> {
    let der = pem_section(cert_pem, "CERTIFICATE")?;
    Some(sha256_hex_colon(&der))
}

#[cfg(target_os = "windows")]
pub(in crate::cli) fn cert_sha1_fingerprint(cert_pem: &str) -> Option<String> {
    use sha1::Sha1;

    let der = pem_section(cert_pem, "CERTIFICATE")?;
    Some(
        Sha1::digest(der)
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect(),
    )
}

fn pem_section(pem: &str, label: &str) -> Option<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut body = String::new();
    let mut inside = false;
    for line in pem.lines() {
        let line = line.trim();
        if line == begin {
            inside = true;
            continue;
        }
        if line == end {
            return general_purpose::STANDARD.decode(body.as_bytes()).ok();
        }
        if inside {
            body.push_str(line);
        }
    }
    None
}

fn sha256_hex_colon(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(target_os = "macos")]
pub(in crate::cli) fn compact_fingerprint(input: &str) -> String {
    input
        .chars()
        .filter(|ch| *ch != ':')
        .collect::<String>()
        .to_ascii_uppercase()
}
