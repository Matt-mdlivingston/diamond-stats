//! One JSON error shape for every non-2xx response.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use diamond_stats_ingest::IngestError;
use diamond_stats_query::QueryError;
use serde::Serialize;
use utoipa::ToSchema;

/// Body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Machine-readable code (`bad_request`, `not_found`, `data_not_loaded`, ...).
    pub error: ErrorDetail,
}

/// Code plus a short message.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable code.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
}

/// Error that becomes a JSON response.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    /// Build an error response.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// 400.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    /// 404.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// 500.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code.to_string(),
                message: self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<QueryError> for ApiError {
    fn from(error: QueryError) -> Self {
        match error {
            QueryError::DataNotLoaded { .. } | QueryError::DatasetMissing { .. } => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "data_not_loaded",
                error.to_string(),
            ),
            QueryError::Message(message)
                if message.contains("limit")
                    || message.contains("requires")
                    || message.contains("date") =>
            {
                Self::bad_request(message)
            }
            QueryError::UnsafePath => Self::bad_request("data directory path is not usable"),
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<IngestError> for ApiError {
    fn from(error: IngestError) -> Self {
        match error {
            IngestError::GameNotFound { game_pk } => {
                Self::not_found(format!("no game {game_pk} in the stats API"))
            }
            IngestError::Status { status: 404, .. } => {
                Self::not_found("the stats API has no row for that request")
            }
            IngestError::Status { .. } | IngestError::Http { .. } => {
                Self::new(StatusCode::BAD_GATEWAY, "upstream", error.to_string())
            }
            other => Self::new(StatusCode::BAD_GATEWAY, "upstream", other.to_string()),
        }
    }
}
