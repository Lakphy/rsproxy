use super::*;

const API_TOKEN_ENV: &str = "RSPROXY_API_TOKEN";
const MIN_API_TOKEN_BYTES: usize = 16;

pub(super) fn prepare_server_api_auth(config: &mut AppConfig) -> Result<(), String> {
    if unix_api_path(&config.api).is_some() {
        config.api_token = None;
        return Ok(());
    }

    let path = api_token_path(&config.storage);
    let token = match config.api_token.take() {
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
    config.api_token = Some(token);
    Ok(())
}

pub(super) fn configure_client_api_auth(args: &[String]) -> Result<(), String> {
    let config = runtime_config(args)?;
    if unix_api_path(&config.api).is_some() {
        set_api_token(None);
        return Ok(());
    }
    let token = option_value(args, "--api-token")
        .or_else(|| env::var(API_TOKEN_ENV).ok())
        .map(|token| validate_api_token(&token))
        .transpose()?
        .or(config.api_token)
        .or_else(|| read_api_token(&api_token_path(&config.storage)).ok());
    set_api_token(token);
    Ok(())
}

pub(super) fn validate_api_token(input: &str) -> Result<String, String> {
    let token = input.trim();
    if token.len() < MIN_API_TOKEN_BYTES {
        return Err(format!(
            "--api-token must be at least {MIN_API_TOKEN_BYTES} bytes"
        ));
    }
    if token.chars().any(char::is_control) {
        return Err("--api-token must not contain control characters".to_string());
    }
    Ok(token.to_string())
}

pub(super) fn api_token_path(storage: &Path) -> PathBuf {
    storage.join("run/api-token")
}

fn read_api_token(path: &Path) -> Result<String, String> {
    let token = fs::read_to_string(path)
        .map_err(|error| format!("read API token {}: {error}", path.display()))?;
    set_private_permissions(path)?;
    validate_api_token(&token)
}

fn write_api_token(path: &Path, token: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("write API token {}: {error}", path.display()))?;
    file.write_all(token.as_bytes())
        .map_err(|error| format!("write API token {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("sync API token {}: {error}", path.display()))?;
    set_private_permissions(path)
}

fn set_private_permissions(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("secure API token {}: {error}", _path.display()))?;
    }
    Ok(())
}

fn generate_api_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| format!("generate API token: {error}"))?;
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
