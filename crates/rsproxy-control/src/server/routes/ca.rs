use super::super::http;
use super::ControlState;
use super::respond_json;
use std::fs;
use std::io::Write;

pub(super) fn root<W: Write + ?Sized>(stream: &mut W, state: &ControlState) -> std::io::Result<()> {
    match fs::read(state.options.storage.join("ca/rsproxy-root-ca.pem")) {
        Ok(body) => http::write_response(
            stream,
            200,
            "OK",
            &[
                (
                    "Content-Type".to_string(),
                    "application/x-x509-ca-cert".to_string(),
                ),
                (
                    "Content-Disposition".to_string(),
                    "attachment; filename=\"rsproxy-root-ca.pem\"".to_string(),
                ),
            ],
            &body,
        ),
        Err(_) => respond_json(stream, 404, "{\"error\":\"ca not initialized\"}"),
    }
}
