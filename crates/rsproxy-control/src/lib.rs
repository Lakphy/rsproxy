// Windows named-pipe transport requires narrowly scoped, documented Win32 FFI.
#![allow(unsafe_code)]

//! Authenticated control-plane transport for an rsproxy engine.
//!
//! The crate owns control endpoint binding, request routing and the matching
//! blocking client. It consumes typed engine and trace handles, but deliberately
//! does not own proxy data-plane policy or CLI presentation.
//!
//! Endpoints without a prefix use TCP. `unix:`/`unix://` selects a Unix-domain
//! socket and `pipe:`/`npipe:` selects a Windows named pipe. Call
//! [`prepare_server_api_auth`] to establish bearer authentication for TCP and
//! named pipes; a Unix socket is created with owner-only permissions and uses
//! local peer access instead of a token.
//!
//! The HTTP/1.1 surface maps `/api/status` to engine and trace snapshots,
//! `/api/rules/*` and `/api/values/*` to persistent configuration resources,
//! `/api/sessions/*` and `/api/trace/*` to trace queries and streaming export,
//! `/api/replay/<id>` to engine replay, and `/api/ca/root.pem` to the injected
//! root certificate. [`client::api_request`] performs one connection-closing
//! request; [`client::api_stream_lines`] consumes newline-delimited follow data.

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
