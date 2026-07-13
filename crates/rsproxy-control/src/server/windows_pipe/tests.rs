use super::{NamedPipeListener, NamedPipeStream};
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn flushed_response_is_read_before_the_server_disconnects() {
    let (result_tx, result_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = result_tx.send(flushed_round_trip());
    });
    let response = result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("named-pipe round trip should finish within ten seconds")
        .expect("named-pipe round trip should succeed");
    assert_eq!(response, b"pong");
}

fn flushed_round_trip() -> io::Result<Vec<u8>> {
    let name = format!(
        "rsproxy-control-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut listener = NamedPipeListener::bind(&name)?;
    let server = std::thread::spawn(move || -> io::Result<()> {
        let mut stream = listener.accept()?;
        let mut request = [0; 4];
        stream.read_exact(&mut request)?;
        if &request != b"ping" {
            return Err(io::Error::other("named-pipe request was corrupted"));
        }
        stream.write_all(b"pong")?;
        stream.flush()
    });

    let mut client = NamedPipeStream::connect(&name)?;
    client.write_all(b"ping")?;
    client.flush()?;
    let mut response = Vec::new();
    client.read_to_end(&mut response)?;
    server
        .join()
        .map_err(|_| io::Error::other("named-pipe server thread panicked"))??;
    Ok(response)
}
