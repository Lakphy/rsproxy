use crate::app::MitmCertCache;
use crate::cli::ca::certificates::{generate_leaf_cert, generate_root_ca};
use crate::proxy::tls::{ensure_leaf_certificate, load_certs, load_private_key};
use rustls::client::Resumption;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct CertificateFixture {
    ca_dir: PathBuf,
}

impl CertificateFixture {
    pub fn create(ca_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let ca_dir = ca_dir.into();
        if ca_dir.exists() {
            fs::remove_dir_all(&ca_dir)?;
        }
        fs::create_dir_all(&ca_dir)?;
        let (certificate, key) =
            generate_root_ca("rsproxy criterion root").map_err(io::Error::other)?;
        fs::write(ca_dir.join("rsproxy-root-ca.pem"), certificate)?;
        fs::write(ca_dir.join("rsproxy-root-ca-key.pem"), key)?;
        Ok(Self { ca_dir })
    }

    pub fn issue_leaf(&self, host: &str) -> io::Result<usize> {
        let (certificate, key, chain) =
            generate_leaf_cert(&self.ca_dir, host).map_err(io::Error::other)?;
        Ok(certificate.len() + key.len() + chain.len())
    }

    pub fn ensure_leaf(&self, host: &str) -> io::Result<(PathBuf, PathBuf)> {
        ensure_leaf_certificate(&self.ca_dir, host)
    }

    pub fn cached_server_config(&self, host: &str) -> io::Result<CachedServerConfig> {
        let (certificate, key) = self.ensure_leaf(host)?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(load_certs(&certificate)?, load_private_key(&key)?)
            .map_err(io::Error::other)?;
        let mut cache = MitmCertCache::new(1_024);
        cache.insert(host.to_string(), Arc::new(config));
        let mut roots = RootCertStore::empty();
        for certificate in load_certs(&self.ca_dir.join("rsproxy-root-ca.pem"))? {
            roots.add(certificate).map_err(io::Error::other)?;
        }
        let mut client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        client_config.resumption = Resumption::disabled();
        Ok(CachedServerConfig {
            cache,
            host: host.to_string(),
            client_config: Arc::new(client_config),
        })
    }
}

impl Drop for CertificateFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.ca_dir);
    }
}

pub struct CachedServerConfig {
    cache: MitmCertCache,
    host: String,
    client_config: Arc<ClientConfig>,
}

impl CachedServerConfig {
    pub fn lookup(&mut self) -> bool {
        self.cache.get(&self.host).is_some()
    }

    pub fn handshake(&mut self) -> io::Result<()> {
        let server_config = self.cache.get(&self.host).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cached server config missing")
        })?;
        let server_name = ServerName::try_from(self.host.clone())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        let mut client = ClientConnection::new(Arc::clone(&self.client_config), server_name)
            .map_err(io::Error::other)?;
        let mut server = ServerConnection::new(server_config).map_err(io::Error::other)?;
        complete_handshake(&mut client, &mut server)
    }
}

fn complete_handshake(
    client: &mut ClientConnection,
    server: &mut ServerConnection,
) -> io::Result<()> {
    for _ in 0..32 {
        let client_bytes = write_client_tls(client)?;
        if !client_bytes.is_empty() {
            read_client_flight(server, &client_bytes)?;
        }

        let server_bytes = write_server_tls(server)?;
        if !server_bytes.is_empty() {
            read_server_flight(client, &server_bytes)?;
        }

        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(());
        }
        if client_bytes.is_empty() && server_bytes.is_empty() {
            break;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "in-memory TLS handshake did not complete",
    ))
}

fn write_client_tls(connection: &mut ClientConnection) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    connection.write_tls(&mut bytes)?;
    Ok(bytes)
}

fn write_server_tls(connection: &mut ServerConnection) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    connection.write_tls(&mut bytes)?;
    Ok(bytes)
}

fn read_client_flight(server: &mut ServerConnection, bytes: &[u8]) -> io::Result<()> {
    let mut reader = bytes;
    while !reader.is_empty() {
        server.read_tls(&mut reader)?;
    }
    server.process_new_packets().map_err(io::Error::other)?;
    Ok(())
}

fn read_server_flight(client: &mut ClientConnection, bytes: &[u8]) -> io::Result<()> {
    let mut reader = bytes;
    while !reader.is_empty() {
        client.read_tls(&mut reader)?;
    }
    client.process_new_packets().map_err(io::Error::other)?;
    Ok(())
}

pub fn fixture_path(name: &str) -> PathBuf {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/criterion-fixtures");
    target.join(format!("{name}-{}", std::process::id()))
}
