use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use std::fmt;
use std::sync::Arc;

use crate::{EngineError, EngineResult};

/// Root CA material injected by the composition root.
///
/// The engine intentionally accepts PEM bytes rather than a platform-owned
/// storage type. Clones share the underlying bytes, and debug output never
/// exposes the private key.
#[derive(Clone, PartialEq, Eq)]
pub struct CaMaterial {
    certificate_pem: Arc<str>,
    private_key_pem: Arc<str>,
}

impl CaMaterial {
    pub fn from_pem(
        certificate_pem: impl Into<Arc<str>>,
        private_key_pem: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            certificate_pem: certificate_pem.into(),
            private_key_pem: private_key_pem.into(),
        }
    }

    pub fn certificate_pem(&self) -> &str {
        &self.certificate_pem
    }

    pub fn private_key_pem(&self) -> &str {
        &self.private_key_pem
    }
}

impl fmt::Debug for CaMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CaMaterial")
            .field("certificate_pem_bytes", &self.certificate_pem.len())
            .field("private_key_pem", &"[REDACTED]")
            .finish()
    }
}

/// PEM material for a leaf certificate signed by an rsproxy root CA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedLeafCertificate {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub chain_pem: String,
}

/// Issues a server-auth leaf certificate from PEM root material.
///
/// Root CA generation, persistence, and trust installation remain platform
/// responsibilities; leaf signing stays in the engine's MITM data path.
pub fn issue_leaf_certificate(
    ca_certificate_pem: &str,
    ca_private_key_pem: &str,
    host: &str,
) -> EngineResult<IssuedLeafCertificate> {
    validate_leaf_host(host)?;
    let ca_key = KeyPair::from_pem(ca_private_key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(ca_certificate_pem, ca_key)?;

    let mut params = CertificateParams::new(vec![host.to_string()])?;
    params.distinguished_name.push(DnType::CommonName, host);
    params.is_ca = IsCa::NoCa;
    params.use_authority_key_identifier_extension = true;
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);

    let key = KeyPair::generate()?;
    let certificate = params.signed_by(&key, &issuer)?;
    let certificate_pem = certificate.pem();
    Ok(IssuedLeafCertificate {
        chain_pem: format!("{certificate_pem}{ca_certificate_pem}"),
        certificate_pem,
        private_key_pem: key.serialize_pem(),
    })
}

fn validate_leaf_host(host: &str) -> EngineResult<()> {
    if host.trim().is_empty() || host.contains('/') || host.chars().any(char::is_whitespace) {
        return Err(EngineError::InvalidInput(format!(
            "invalid certificate host `{host}`"
        )));
    }
    Ok(())
}
