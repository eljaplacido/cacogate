//! Tiny blocking client for the daemon's loopback API.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// Client errors.
#[derive(Debug, thiserror::Error)]
pub(crate) enum HttpError {
    /// Transport failure (daemon down, timeout).
    #[error("daemon unreachable at {0}: {1}")]
    Unreachable(String, String),
    /// Non-2xx answer.
    #[error("daemon answered {0}: {1}")]
    Status(u16, String),
    /// Bad JSON.
    #[error("bad response: {0}")]
    Decode(String),
}

/// Daemon client.
pub(crate) struct Client {
    base: String,
    agent: ureq::Agent,
}

impl Client {
    /// New client with a per-request timeout.
    pub(crate) fn new(addr: &str, timeout: Duration) -> Client {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        Client {
            base: format!("http://{addr}"),
            agent,
        }
    }

    fn map(&self, e: ureq::Error) -> HttpError {
        match e {
            ureq::Error::Status(code, resp) => {
                HttpError::Status(code, resp.into_string().unwrap_or_default())
            }
            ureq::Error::Transport(t) => HttpError::Unreachable(self.base.clone(), t.to_string()),
        }
    }

    /// GET a JSON document.
    pub(crate) fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, HttpError> {
        let r = self
            .agent
            .get(&format!("{}{path}", self.base))
            .call()
            .map_err(|e| self.map(e))?;
        r.into_json().map_err(|e| HttpError::Decode(e.to_string()))
    }

    /// POST JSON, decode JSON.
    pub(crate) fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, HttpError> {
        let r = self
            .agent
            .post(&format!("{}{path}", self.base))
            .send_json(body)
            .map_err(|e| self.map(e))?;
        r.into_json().map_err(|e| HttpError::Decode(e.to_string()))
    }
}
