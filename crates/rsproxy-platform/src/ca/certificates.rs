use crate::PlatformResult;
use base64::{Engine as _, engine::general_purpose};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use sha2::{Digest, Sha256};

/// PEM-encoded root certificate material owned by the platform layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootCaPem {
    /// PEM-encoded X.509 certificate suitable for trust-store installation.
    pub certificate_pem: String,
    /// PEM-encoded private key that must remain confined to user-controlled storage.
    pub private_key_pem: String,
}

/// Generates a self-signed root CA. Leaf issuance intentionally lives in
/// `rsproxy-engine` because it is part of the MITM data path.
pub fn generate_root_ca(common_name: &str) -> PlatformResult<RootCaPem> {
    let mut params = CertificateParams::default();
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, common_name);
    distinguished_name.push(DnType::OrganizationName, "rsproxy");
    params.distinguished_name = distinguished_name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::CrlSign);
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);

    let key = KeyPair::generate()?;
    let certificate = params.self_signed(&key)?;
    Ok(RootCaPem {
        certificate_pem: certificate.pem(),
        private_key_pem: key.serialize_pem(),
    })
}

/// Computes the uppercase, colon-delimited SHA-256 fingerprint of the first PEM certificate.
///
/// Returns `None` when a certificate section is absent or its base64 body is invalid.
pub fn certificate_fingerprint_sha256(certificate_pem: &str) -> Option<String> {
    let der = pem_section(certificate_pem, "CERTIFICATE")?;
    Some(hex_fingerprint(&Sha256::digest(der), ":"))
}

#[cfg(target_os = "windows")]
pub(crate) fn certificate_fingerprint_sha1(certificate_pem: &str) -> Option<String> {
    use sha1::Sha1;

    let der = pem_section(certificate_pem, "CERTIFICATE")?;
    Some(hex_fingerprint(&Sha1::digest(der), ""))
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

fn hex_fingerprint(input: &[u8], separator: &str) -> String {
    input
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(separator)
}

#[cfg(target_os = "macos")]
pub(crate) fn compact_fingerprint(input: &str) -> String {
    input
        .chars()
        .filter(|character| *character != ':')
        .collect::<String>()
        .to_ascii_uppercase()
}
