use crate::CliResult;
use rsproxy_engine::{CaMaterial, ProxyConfig};
use std::env;
use std::fmt;
use std::path::PathBuf;

pub(crate) use rsproxy_control::{unix_api_path, windows_pipe_path};

/// Composition-root configuration.
///
/// Runtime proxy settings live in `ProxyConfig`; this wrapper retains only the
/// listener/control metadata and CLI precedence state owned by the executable.
#[derive(Clone)]
pub(crate) struct AppConfig {
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) api: String,
    pub(crate) api_token: Option<String>,
    engine: ProxyConfig,
}

impl AppConfig {
    pub(crate) fn engine(&self) -> &ProxyConfig {
        &self.engine
    }

    pub(crate) fn engine_mut(&mut self) -> &mut ProxyConfig {
        &mut self.engine
    }

    fn proxy_config(&self) -> ProxyConfig {
        self.engine.clone()
    }

    /// Builds the data-plane configuration at the composition boundary.
    ///
    /// Platform storage owns root CA discovery. The engine receives only the
    /// explicitly injected PEM material and never knows the root file names.
    pub(crate) fn proxy_config_with_ca_material(&self) -> CliResult<ProxyConfig> {
        let mut engine = self.proxy_config();
        engine.ca_material = None;
        let ca_directory = self.engine.storage.join("ca");
        if rsproxy_platform::ca::root_ca_status(&ca_directory)?.initialized {
            let root = rsproxy_platform::ca::read_root_ca(&ca_directory)?;
            engine.ca_material = Some(CaMaterial::from_pem(
                root.certificate_pem,
                root.private_key_pem,
            ));
        }
        Ok(engine)
    }

    pub(crate) fn control_options(&self) -> rsproxy_control::ControlOptions {
        rsproxy_control::ControlOptions {
            host: self.host.clone(),
            port: self.port,
            api: self.api.clone(),
            api_token: self.api_token.clone(),
            storage: self.engine.storage.clone(),
            config_path: self.config_path.clone(),
            rules_watch: self.engine.rules_watch,
            rules_watch_debounce: self.engine.rules_watch_debounce,
            max_header_size: self.engine.max_header_size,
            max_header_count: self.engine.max_header_count,
            max_body_size: self.engine.body_buffer_limit,
        }
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("config_path", &self.config_path)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("api", &self.api)
            .field("api_token", &self.api_token.as_ref().map(|_| "[REDACTED]"))
            .field("storage", &self.engine.storage)
            .field("engine", &"[REDACTED CONFIG]")
            .finish()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        let storage = default_storage();
        Self {
            config_path: None,
            host: "127.0.0.1".to_string(),
            port: 8899,
            api: default_api_for_storage(&storage),
            api_token: None,
            engine: ProxyConfig::new(storage),
        }
    }
}

pub(crate) fn default_api_for_storage(storage: &std::path::Path) -> String {
    #[cfg(windows)]
    {
        let _ = storage;
        "pipe:rsproxy-control".to_string()
    }
    #[cfg(unix)]
    {
        format!(
            "unix:{}",
            rsproxy_platform::process::unix_control_socket_path(storage).display()
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = storage;
        "127.0.0.1:8900".to_string()
    }
}

pub(crate) fn api_display(api: &str) -> String {
    if unix_api_path(api).is_some() || windows_pipe_path(api).is_some() {
        api.to_string()
    } else {
        format!("http://{api}")
    }
}

pub(crate) fn default_storage() -> PathBuf {
    env::var_os("RSPROXY_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".rsproxy")))
        .unwrap_or_else(|| PathBuf::from(".rsproxy"))
}
