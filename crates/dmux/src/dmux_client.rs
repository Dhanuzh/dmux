use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use crate::dmux_proto::{Request, Response};

pub fn send_request(socket_path: &Path, request: Request) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;

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
