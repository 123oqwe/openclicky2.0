//! Blocking HTTP with rustls and an SSE line reader.

use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use openclicky_core::error::{Error, Result};
use openclicky_core::ports::*;

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                // A redirect could bounce a checked public URL to 127.0.0.1.
                // Follow none; callers see the 3xx and decide.
                .redirects(0)
                // No read timeout here on purpose. Every call already passes its
                // own `Request::timeout`, and that one covers the whole call.
                // An agent-level read timeout of 30s was shorter than the 180s a
                // chat request asks for, so a model that thought for more than
                // thirty seconds before its first byte was reported as a failure
                // with 150 seconds still on the clock. Two timeouts where the
                // shorter one wins is one timeout too many.
                .build(),
        }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

fn map(e: ureq::Error) -> Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            Error::BadRequest(format!(
                "HTTP {code}: {}",
                body.chars().take(200).collect::<String>()
            ))
        }
        ureq::Error::Transport(t) => {
            let msg = t.to_string();
            if msg.contains("timed out") || msg.contains("timeout") {
                Error::Timeout
            } else {
                Error::Network(msg)
            }
        }
    }
}

fn into_response(result: std::result::Result<ureq::Response, ureq::Error>) -> Result<HttpResponse> {
    match result {
        Ok(resp) => read_response(resp),
        Err(ureq::Error::Status(_, resp)) => read_response(resp),
        Err(e) => Err(map(e)),
    }
}

fn read_response(resp: ureq::Response) -> Result<HttpResponse> {
    let status = resp.status();
    let headers: Vec<(String, String)> = resp
        .headers_names()
        .into_iter()
        .filter_map(|k| resp.header(&k).map(|v| (k.clone(), v.to_string())))
        .collect();
    let mut body: Vec<u8> = Vec::new();
    resp.into_reader()
        .take(8 * 1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|e| Error::Network(e.to_string()))?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

impl HttpTransport for UreqTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeout: Duration,
    ) -> Result<HttpResponse> {
        let mut req = self.agent.get(url).timeout(timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        into_response(req.call())
    }

    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<HttpResponse> {
        let mut req = self.agent.post(url).timeout(timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        into_response(req.send_string(&body.to_string()))
    }

    fn post_multipart(
        &self,
        url: &str,
        headers: &[(String, String)],
        form: &MultipartForm,
        timeout: Duration,
    ) -> Result<HttpResponse> {
        let boundary = format!(
            "----openclicky{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let mut body: Vec<u8> = Vec::new();
        for (name, value) in &form.fields {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        for f in &form.files {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                    f.field, f.filename, f.content_type
                )
                .as_bytes(),
            );
            body.extend_from_slice(&f.bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let mut req = self.agent.post(url).timeout(timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        req = req.set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        );
        into_response(req.send_bytes(&body))
    }

    fn post_json_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<Box<dyn LineStream>> {
        let mut req = self.agent.post(url).timeout(timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = match req.send_string(&body.to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(Error::BadRequest(format!(
                    "HTTP {code}: {}",
                    body.chars().take(200).collect::<String>()
                )));
            }
            Err(e) => return Err(map(e)),
        };
        let reader: Box<dyn Read + Send + Sync + 'static> = Box::new(resp.into_reader());
        Ok(Box::new(SseReader {
            lines: BufReader::new(reader).lines(),
        }))
    }
}

struct SseReader {
    lines: std::io::Lines<BufReader<Box<dyn Read + Send + Sync + 'static>>>,
}

impl LineStream for SseReader {
    fn next_line(&mut self) -> Result<Option<String>> {
        match self.lines.next() {
            None => Ok(None),
            Some(Ok(l)) => Ok(Some(l)),
            Some(Err(e)) => Err(Error::Network(e.to_string())),
        }
    }
}
