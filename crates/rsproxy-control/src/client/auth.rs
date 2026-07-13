use crate::server::unix_api_path;
use crate::{ControlError, ControlResult};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MIN_API_TOKEN_BYTES: usize = 16;

/// Prepares authentication for a non-Unix control server before it is bound.
///
/// Unix sockets clear `token` because owner-only socket permissions are the
/// access boundary. Other transports validate and persist an explicit token,
/// reuse the token file when present, or generate a new 32-byte random token.
pub fn prepare_server_api_auth(
    api: &str,
    storage: &Path,
    token: &mut Option<String>,
) -> ControlResult<()> {
    if unix_api_path(api).is_some() {
        *token = None;
        return Ok(());
    }

    let path = api_token_path(storage);
    let prepared = match token.take() {
        Some(token) => {
            let token = validate_api_token(&token)?;
            write_api_token(&path, &token)?;
            token
        }
        None if path.is_file() => read_api_token(&path)?,
        None => {
            let token = generate_api_token()?;
            write_api_token(&path, &token)?;
            token
        }
    };
    *token = Some(prepared);
    Ok(())
}

/// Resolves a client token using explicit, environment, configured, then stored values.
///
/// Unix endpoints always return `None`. A missing stored token is also `None`;
/// malformed or unreadable existing token files remain errors.
pub fn resolve_client_api_token(
    api: &str,
    storage: &Path,
    explicit: Option<String>,
    environment: Option<String>,
    configured: Option<String>,
) -> ControlResult<Option<String>> {
    if unix_api_path(api).is_some() {
        return Ok(None);
    }

    if let Some(token) = explicit.or(environment).or(configured) {
        return validate_api_token(&token).map(Some);
    }
    match read_api_token(&api_token_path(storage)) {
        Ok(token) => Ok(Some(token)),
        Err(error) => match &error {
            ControlError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            _ => Err(error),
        },
    }
}

/// Trims and validates a bearer token of at least 16 UTF-8 bytes with no control characters.
pub fn validate_api_token(input: &str) -> ControlResult<String> {
    let token = input.trim();
    if token.len() < MIN_API_TOKEN_BYTES {
        return Err(ControlError::Authentication(format!(
            "--api-token must be at least {MIN_API_TOKEN_BYTES} bytes"
        )));
    }
    if token.chars().any(char::is_control) {
        return Err(ControlError::Authentication(
            "--api-token must not contain control characters".to_string(),
        ));
    }
    Ok(token.to_string())
}

/// Returns `<storage>/run/api-token`, whose Unix permissions are normalized to `0600`.
pub fn api_token_path(storage: &Path) -> PathBuf {
    storage.join("run/api-token")
}

fn read_api_token(path: &Path) -> ControlResult<String> {
    let token = fs::read_to_string(path)
        .map_err(|source| ControlError::io(format!("read API token {}", path.display()), source))?;
    set_private_permissions(path)?;
    validate_api_token(&token)
}

fn write_api_token(path: &Path, token: &str) -> ControlResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            ControlError::io(
                format!("create API token directory {}", parent.display()),
                source,
            )
        })?;
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| {
        ControlError::io(format!("write API token {}", path.display()), source)
    })?;
    file.write_all(token.as_bytes()).map_err(|source| {
        ControlError::io(format!("write API token {}", path.display()), source)
    })?;
    file.sync_all()
        .map_err(|source| ControlError::io(format!("sync API token {}", path.display()), source))?;
    set_private_permissions(path)
}

fn set_private_permissions(_path: &Path) -> ControlResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ControlError::io(format!("secure API token {}", _path.display()), source)
        })?;
    }
    Ok(())
}

fn generate_api_token() -> ControlResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|source| ControlError::Random {
        context: "generate API token".to_string(),
        source,
    })?;
    Ok(hex_lower(&bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
