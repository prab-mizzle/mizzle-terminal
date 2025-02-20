use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_error_derive::HttpError;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
pub struct ContainerBindingResponse {
    pub session_id: Option<String>,
    pub url: Option<String>,
    pub status: BindingStatus,
}

#[derive(Serialize, Deserialize, Debug, HttpError)]
pub enum BindingStatus {
    Binding,
    Failed(String),
    Live,
    Error(String),
    #[http(code = 500, message = "Failed to bind the port")]
    PortAllocFailed(String),
    #[http(code = 500, message = "Error reading process value")]
    ProcessReadError(String),
    #[http(code = 500, message = "Error port number not found")]
    PortNotFound(String),
    #[http(code = 400, message = "Process already attached: url {0}")]
    SessionRunning(String),
}

impl IntoResponse for BindingStatus {
    fn into_response(self) -> Response<Body> {
        let body = Body::new(self.http_message().unwrap_or("Some Error").to_string());
        let mut resp = Response::new(body);
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        resp
    }
}

// Implement `IntoResponse`
impl IntoResponse for ContainerBindingResponse {
    fn into_response(self) -> Response {
        match serde_json::to_string(&self) {
            Ok(json_body) => {
                let mut headers = axum::http::HeaderMap::new();
                headers.insert(
                    "Content-Type",
                    axum::http::HeaderValue::from_static("application/json"),
                );

                (StatusCode::OK, headers, json_body).into_response()
            }
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to serialize response",
            )
                .into_response(),
        }
    }
}
