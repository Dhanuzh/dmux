use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use crate::dmux_proto::{Request, Response};

fn try_autostart_server(socket_path: &Path) -> Result<()> {
    if let Ok(exe) = std::env::current_exe() {
        // Detach: child becomes its own session leader.
        let _ = std::process::Command::new(exe)
            .arg("--socket")
            .arg(socket_path)
            .arg("start-server")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if UnixStream::connect(socket_path).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    Err(anyhow!("server did not start on {}", socket_path.display()))
}

pub fn send_request(socket_path: &Path, request: Request) -> Result<Response> {
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(err) if matches!(err.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound) => {
            try_autostart_server(socket_path)?;
            UnixStream::connect(socket_path)
                .with_context(|| format!("failed to connect after autostart {}", socket_path.display()))?
        }
        Err(err) => return Err(anyhow!("connect {}: {err}", socket_path.display())),
    };

    let wire = serde_json::to_string(&request).context("failed to encode request")?;
    stream
        .write_all(wire.as_bytes())
        .context("failed to write request")?;
    stream.write_all(b"\n").context("failed to write newline")?;
    stream.flush().context("failed to flush request")?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read response")?;
    if bytes == 0 {
        return Err(anyhow!("server closed connection without a response"));
    }

    let response: Response =
        serde_json::from_str(line.trim_end()).context("failed to decode response JSON")?;
    Ok(response)
}
