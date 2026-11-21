use std::io::{self, Read};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Error, Host, Result};

pub(crate) const BOSE_PORT: u16 = 8090;
pub(crate) const STR_PORTS: [u16; 2] = [8888, 17008];

/// A shared deadline and cancellation flag for an initial read-only connection.
#[derive(Clone, Debug)]
pub struct ConnectionAttempt {
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

impl ConnectionAttempt {
    pub fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn remaining(&self) -> Result<Duration> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::ConnectionCancelled);
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(Error::ConnectionTimeout)
        } else {
            Ok(remaining)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Method {
    Get,
    Put,
    Delete,
}

impl Method {
    fn label(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

pub(crate) trait Transport: Send + Sync {
    fn request(
        &self,
        host: Host,
        port: u16,
        method: Method,
        path: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>>;
}

pub(crate) struct HttpTransport {
    read: ureq::Agent,
    write: ureq::Agent,
    attempt: Option<ConnectionAttempt>,
}

impl Default for HttpTransport {
    fn default() -> Self {
        fn agent(seconds: u64) -> ureq::Agent {
            ureq::Agent::config_builder()
                .proxy(None)
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(4)))
                .timeout_global(Some(Duration::from_secs(seconds)))
                .user_agent(concat!("soundtouch-radio/", env!("CARGO_PKG_VERSION")))
                .build()
                .into()
        }
        Self {
            read: agent(4),
            write: agent(45),
            attempt: None,
        }
    }
}

impl HttpTransport {
    pub(crate) fn for_connection(attempt: ConnectionAttempt) -> Self {
        Self {
            attempt: Some(attempt),
            ..Self::default()
        }
    }
}

impl Transport for HttpTransport {
    fn request(
        &self,
        host: Host,
        port: u16,
        method: Method,
        path: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>> {
        let url = format!("http://{host}:{port}{path}");
        let operation = format!("{} {port}{path}", method.label());
        let write = !matches!(method, Method::Get);
        let mut response = match method {
            Method::Get => {
                let request = self.read.get(&url);
                match &self.attempt {
                    Some(attempt) => request
                        .config()
                        .timeout_global(Some(attempt.remaining()?.min(Duration::from_secs(4))))
                        .build()
                        .call(),
                    None => request.call(),
                }
            }
            Method::Put => self
                .write
                .put(&url)
                .header("Content-Type", "application/json")
                .send(payload),
            Method::Delete => self.write.delete(&url).call(),
        }
        .map_err(|err| {
            let reason = match err {
                ureq::Error::Timeout(_) => "connection or response timed out",
                ureq::Error::Io(io) if io.kind() == io::ErrorKind::ConnectionRefused => {
                    "connection refused"
                }
                _ => "network connection failed",
            };
            let reason = if write {
                format!("{reason}. Read back the presets before retrying")
            } else {
                reason.into()
            };
            Error::Network {
                operation: operation.clone(),
                reason,
            }
        })?;

        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(Error::Response(format!("{operation} returned a redirect")));
        }
        if status == 204 && matches!(method, Method::Delete) {
            return Ok(Vec::new());
        }
        let limit = if port == BOSE_PORT {
            256 * 1024
        } else {
            1024 * 1024
        };
        let mut data = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|_| Error::Response(format!("{operation} returned an incomplete response")))?;
        if data.len() as u64 > limit {
            return Err(Error::Response(format!(
                "{operation} response exceeds the size limit"
            )));
        }
        if status == 200 {
            return Ok(data);
        }
        let reply: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        let code = reply["code"]
            .as_str()
            .filter(|s| {
                (1..=64).contains(&s.len())
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            .map(str::to_owned);
        let slot = reply["slot"]
            .as_u64()
            .filter(|slot| (1..=12).contains(slot))
            .map(|slot| slot as u8);
        let mut detail = code
            .as_deref()
            .map(|code| format!(" ({code})"))
            .unwrap_or_default();
        if let Some(slot) = slot {
            detail.push_str(&format!(", slot {slot}"));
        }
        Err(Error::Http {
            operation,
            status,
            detail,
            code,
            slot,
        })
    }
}
