//! Minimal Server-Sent-Events reader shared by the streaming LLM clients.
//! Both Gemini (`?alt=sse`) and Anthropic (`"stream": true`) emit
//! `data: {json}` lines; everything else (event names, comments, blank
//! keep-alives) can be ignored for our purposes.

use futures_util::StreamExt;

use crate::error::{Error, Result};

/// Drive an SSE response body, invoking `on_data` for every `data:` payload
/// line. `on_data` may return an error to abort the stream (e.g. a refusal
/// event).
pub async fn for_each_data<F>(resp: reqwest::Response, mut on_data: F) -> Result<()>
where
    F: FnMut(&str) -> Result<()>,
{
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(Error::Http)?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim_start();
                if !data.is_empty() && data != "[DONE]" {
                    on_data(data)?;
                }
            }
        }
    }
    // Trailing data without a final newline.
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim_start();
            if !data.is_empty() && data != "[DONE]" {
                on_data(data)?;
            }
        }
    }
    Ok(())
}
