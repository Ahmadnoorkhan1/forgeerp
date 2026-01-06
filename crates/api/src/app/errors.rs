use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use forgeerp_accounting::AccountKind;
use forgeerp_infra::command_dispatcher::DispatchError;

pub fn dispatch_error_to_response(err: DispatchError) -> axum::response::Response {
    match err {
        DispatchError::Concurrency(msg) => json_error(StatusCode::CONFLICT, "conflict", msg),
        DispatchError::Validation(msg) => json_error(StatusCode::BAD_REQUEST, "validation_error", msg),
        DispatchError::InvariantViolation(msg) => {
            json_error(StatusCode::UNPROCESSABLE_ENTITY, "invariant_violation", msg)
        }
        DispatchError::Unauthorized => json_error(StatusCode::FORBIDDEN, "unauthorized", "unauthorized"),
        DispatchError::NotFound => json_error(StatusCode::NOT_FOUND, "not_found", "not found"),
        DispatchError::Deserialize(msg) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "deserialize_error", msg)
        }
        DispatchError::Store(e) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            format!("{e:?}"),
        ),
        DispatchError::Publish(msg) => json_error(StatusCode::BAD_GATEWAY, "publish_error", msg),
        DispatchError::TenantIsolation(msg) => json_error(StatusCode::FORBIDDEN, "tenant_isolation", msg),
    }
}

pub fn json_error(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> axum::response::Response {
    (
        status,
        axum::Json(json!({
            "error": code,
            "message": message.into(),
        })),
    )
        .into_response()
}

pub fn parse_account_kind(s: &str) -> Result<AccountKind, axum::response::Response> {
    match s.to_lowercase().as_str() {
        "asset" => Ok(AccountKind::Asset),
        "liability" => Ok(AccountKind::Liability),
        "equity" => Ok(AccountKind::Equity),
        "revenue" => Ok(AccountKind::Revenue),
        "expense" => Ok(AccountKind::Expense),
        _ => Err(json_error(
            StatusCode::BAD_REQUEST,
            "invalid_account_kind",
            "kind must be one of: asset, liability, equity, revenue, expense",
        )),
    }
}


