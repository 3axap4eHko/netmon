use std::time::Instant;

use crate::types::HTTP_ENDPOINTS;

pub struct HttpProber {
    client: reqwest::blocking::Client,
}

impl HttpProber {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create HTTP probe client: {}", e))?;

        Ok(Self { client })
    }

    /// Send a probe for the given endpoint key and return RTT in milliseconds.
    pub fn probe(&self, target_key: &str) -> Option<f64> {
        let endpoint = HTTP_ENDPOINTS.iter().find(|e| e.key == target_key)?;
        let body = vec![0u8; endpoint.payload_size];

        let start = Instant::now();
        let resp = self
            .client
            .post(endpoint.url)
            .body(body)
            .send();

        match resp {
            Ok(r) if r.status().is_success() || r.status().is_redirection() => {
                Some(start.elapsed().as_secs_f64() * 1000.0)
            }
            Ok(r) => {
                eprintln!("HTTP probe {} got status {}", target_key, r.status());
                // Still report RTT — the TCP handshake + upload happened
                Some(start.elapsed().as_secs_f64() * 1000.0)
            }
            Err(e) => {
                eprintln!("HTTP probe {} failed: {}", target_key, e);
                None
            }
        }
    }
}
