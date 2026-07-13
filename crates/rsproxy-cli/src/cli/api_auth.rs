use super::command::{ClientArgs, RuntimeArgs};
use super::config::runtime_config;
use crate::CliResult;
use crate::app::AppConfig;
use rsproxy_control::set_api_token;
use std::env;

const API_TOKEN_ENV: &str = "RSPROXY_API_TOKEN";

pub(super) use rsproxy_control::{api_token_path, validate_api_token};

pub(super) fn prepare_server_api_auth(config: &mut AppConfig) -> CliResult<()> {
    let api = config.api.clone();
    let storage = config.engine().storage.clone();
    rsproxy_control::prepare_server_api_auth(&api, &storage, &mut config.api_token)?;
    Ok(())
}

pub(super) fn configure_client_api_auth(args: &ClientArgs) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.clone()))?;
    let token = rsproxy_control::resolve_client_api_token(
        &config.api,
        &config.engine().storage,
        args.api_token.clone(),
        env::var(API_TOKEN_ENV).ok(),
        config.api_token.clone(),
    )?;
    set_api_token(token);
    Ok(())
}
