use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use reqwest::{
    Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER},
};

use crate::{EMOpenjevHttpTiming, Evaluation, Request};

/// Reusable asynchronous HTTP client. Secrets are neither logged nor debug-printed.
#[derive(Clone)]
pub struct UOpenjevClient {
    http: reqwest::Client,
    endpoint: Url,
}
pub type Client = UOpenjevClient;

impl Client {
    pub fn new(api_key: &str, base_url: &str, timeout: Duration) -> Result<Self> {
        ensure!(
            !api_key.trim().is_empty(),
            "Configure TYPESAFE_TOKEN in config/.secrets/secret_env.toml or start with --demo"
        );
        ensure!(!timeout.is_zero(), "Timeout must be greater than zero");
        let base = Url::parse(base_url).context("Invalid TYPESAFE_BASE_URL")?;
        let local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(
            base.scheme() == "https" || (base.scheme() == "http" && local),
            "Use HTTPS; HTTP is only allowed on localhost for testing"
        );
        ensure!(
            base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none(),
            "The URL must not contain credentials, query parameters, or fragments"
        );
        let endpoint = Url::parse(&format!(
            "{}/v1/systemone",
            base.as_str().trim_end_matches('/')
        ))?;
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
            .map_err(|_| anyhow::anyhow!("Invalid API key format"))?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(concat!("openjev/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            .build()?;
        Ok(Self { http, endpoint })
    }

    /// Retries only explicit rate-limit/overload replies (two retries maximum).
    /// Connection failures are not replayed: the server may already have billed them.
    pub async fn evaluate(&self, request: &Request) -> Result<Evaluation> {
        self.evaluate_timed(request)
            .await
            .map(|(response, _)| response)
    }

    /// TTFB measures the first nonempty byte of the successful response body.
    /// This API has no token stream, so it cannot expose a meaningful TTFT.
    pub async fn evaluate_timed(
        &self,
        request: &Request,
    ) -> Result<(Evaluation, EMOpenjevHttpTiming)> {
        let start = Instant::now();
        request.validate()?;
        for attempt in 0..3_u32 {
            let mut response = self
                .http
                .post(self.endpoint.clone())
                .json(request)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        anyhow::anyhow!("TypeSafe timeout: retry or increase --timeout-secs")
                    } else {
                        anyhow::anyhow!("TypeSafe connection failed: check the network and URL")
                    }
                })?;
            let status = response.status().as_u16();
            if matches!(status, 429 | 529) && attempt < 2 {
                let delay = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .min(30);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            }
            if !response.status().is_success() {
                let hint = match status {
                    401 | 403 => "invalid API key or unauthorized access",
                    422 => "request rejected: check the model, state, and criteria",
                    429 => "rate limit reached; retry later",
                    529 => "service overloaded; retry later",
                    _ => "the service did not complete the request",
                };
                bail!("HTTP {status}: {hint}");
            }
            const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
            ensure!(
                response
                    .content_length()
                    .is_none_or(|size| size <= MAX_RESPONSE_BYTES as u64),
                "TypeSafe response exceeds the 8 MiB safety limit"
            );
            let mut body = Vec::new();
            let mut first_byte_ms = None;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| anyhow::anyhow!("TypeSafe response interrupted or timed out"))?
            {
                if !chunk.is_empty() && first_byte_ms.is_none() {
                    first_byte_ms = Some(start.elapsed().as_millis());
                }
                ensure!(
                    chunk.len() <= MAX_RESPONSE_BYTES.saturating_sub(body.len()),
                    "TypeSafe response exceeds the 8 MiB safety limit"
                );
                body.extend_from_slice(&chunk);
            }
            // Parsing errors can include server-provided strings. Do not echo them.
            let evaluation: Evaluation = serde_json::from_slice(&body)
                .map_err(|_| anyhow::anyhow!("Invalid TypeSafe JSON response"))?;
            evaluation
                .validate_for(request)
                .context("TypeSafe response does not match the request")?;
            return Ok((
                evaluation,
                EMOpenjevHttpTiming {
                    first_byte_ms,
                    total_ms: start.elapsed().as_millis(),
                },
            ));
        }
        unreachable!("all attempts return or retry")
    }
}
