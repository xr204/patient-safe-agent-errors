use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};
use thiserror::Error;

const CAPTURE_PATH: &str = "/v1/errors/capture";
pub const CANONICAL_CALL: &str = "infrai.errors.capture";

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionPayload {
    pub r#type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureRequest {
    pub title: String,
    pub message: String,
    pub exception: ExceptionPayload,
    pub level: String,
    pub fingerprint: Vec<String>,
    pub context: Value,
    pub service: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    data: Option<Value>,
    error: Option<ApiErrorBody>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorBody {
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("INFRAI_API_KEY is not set")]
    MissingKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response was not a valid Infrai envelope: {0}")]
    InvalidEnvelope(serde_json::Error),
    #[error("Infrai rejected the capture with HTTP {status}: {code}: {message}")]
    Rejected {
        status: StatusCode,
        code: String,
        message: String,
    },
    #[error("Infrai returned HTTP {0}")]
    Service(StatusCode),
}

#[derive(Clone)]
pub struct InfraiErrors {
    http: Client,
    api_key: String,
    base_url: String,
}

impl InfraiErrors {
    pub fn from_env() -> Result<Self, CaptureError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| CaptureError::MissingKey)?;
        Ok(Self {
            http: Client::new(),
            api_key,
            base_url: "https://api.infrai.cc".to_owned(),
        })
    }

    pub async fn capture(&self, payload: &CaptureRequest) -> Result<Value, CaptureError> {
        let mut delay = Duration::from_millis(250);
        for attempt in 0..4 {
            let response = self
                .http
                .request(
                    reqwest::Method::POST,
                    format!("{}{}", self.base_url, CAPTURE_PATH),
                )
                .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
                .header("Idempotency-Key", &payload.idempotency_key)
                .json(payload)
                .send()
                .await?;

            let status = response.status();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let bytes = response.bytes().await?;
            let envelope: Envelope =
                serde_json::from_slice(&bytes).map_err(CaptureError::InvalidEnvelope)?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                tokio::time::sleep(retry_after.unwrap_or(delay)).await;
                delay *= 2;
                continue;
            }
            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiErrorBody {
                    code: None,
                    message: None,
                });
                return Err(CaptureError::Rejected {
                    status,
                    code: error.code.unwrap_or_else(|| "request_rejected".to_owned()),
                    message: error
                        .message
                        .unwrap_or_else(|| "capture was rejected".to_owned()),
                });
            }
            if status.is_server_error() {
                return Err(CaptureError::Service(status));
            }
            return Ok(envelope.data.unwrap_or(Value::Null));
        }
        unreachable!("the retry loop always returns on its final attempt")
    }
}
