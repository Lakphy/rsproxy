// Windows named-pipe transport requires narrowly scoped, documented Win32 FFI.
#![allow(unsafe_code)]

pub mod client;
mod error;
pub mod server;
mod shapes;

pub use client::{
    api_request, api_stream_lines, api_token_path, prepare_server_api_auth,
    resolve_client_api_token, set_api_token, validate_api_token,
};
pub use error::{ControlError, ControlResult};
pub use server::{
    ControlListener, ControlOptions, ControlState, bind, serve, unix_api_path, windows_pipe_path,
};
