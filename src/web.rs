use crate::alerts::store::{AlertSort, AlertStore};
use crate::error::AlertError;
use crate::triggers::TriggerContext;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;

const INDEX_HTML: &str = include_str!("../statics/index.html");
const STYLES_CSS: &str = include_str!("../statics/styles.css");
const APP_JS: &str = include_str!("../statics/app.js");
const FAVICON_SVG: &str = include_str!("../statics/favicon.svg");

#[derive(Clone)]
struct AppState {
    database_path: PathBuf,
    manual_analysis_sender: Option<SyncSender<ManualAnalysisRequest>>,
}

#[derive(Debug)]
pub struct ManualAnalysisRequest {
    pub candidate_id: i64,
    pub context: TriggerContext,
}

#[derive(Debug, Deserialize)]
struct AlertQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    sort: Option<String>,
    order: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Serialize)]
struct AnalysisQueued {
    candidate_id: i64,
    analysis_status: &'static str,
}

pub fn spawn(
    database_path: PathBuf,
    bind_address: SocketAddr,
    shutdown: Receiver<()>,
    manual_analysis_sender: SyncSender<ManualAnalysisRequest>,
) -> std::io::Result<JoinHandle<()>> {
    let listener = std::net::TcpListener::bind(bind_address)?;
    listener.set_nonblocking(true)?;

    Ok(std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                error!("Failed to create web server runtime: {error}");
                return;
            }
        };

        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    error!("Failed to initialize web server listener: {error}");
                    return;
                }
            };
            let app = router(database_path, Some(manual_analysis_sender));
            info!("Alert dashboard available at http://{bind_address}");
            let shutdown_signal = async move {
                let _ = tokio::task::spawn_blocking(move || shutdown.recv()).await;
            };

            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await
            {
                error!("Web server stopped unexpectedly: {error}");
            }
        });
    }))
}

fn router(
    database_path: PathBuf,
    manual_analysis_sender: Option<SyncSender<ManualAnalysisRequest>>,
) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/alerts", get(alerts))
        .route("/api/alerts/{candidate_id}", get(alert_details))
        .route(
            "/api/alerts/{candidate_id}/analyze",
            post(analyze_candidate),
        )
        .route("/assets/styles.css", get(styles))
        .route("/assets/app.js", get(script))
        .route("/favicon.svg", get(favicon))
        .with_state(AppState {
            database_path,
            manual_analysis_sender,
        })
}

async fn index() -> Response {
    let mut response = Html(INDEX_HTML).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn styles() -> Response {
    static_asset("text/css; charset=utf-8", STYLES_CSS)
}

async fn script() -> Response {
    static_asset("text/javascript; charset=utf-8", APP_JS)
}

async fn favicon() -> Response {
    static_asset("image/svg+xml", FAVICON_SVG)
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn alerts(
    State(state): State<AppState>,
    Query(query): Query<AlertQuery>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(10).clamp(5, 50);
    let sort = match query.sort.as_deref() {
        Some("severity") => AlertSort::Severity,
        Some("status") => AlertSort::Status,
        Some("summary") => AlertSort::Summary,
        _ => AlertSort::AssessedAt,
    };
    let descending = !matches!(query.order.as_deref(), Some("asc"));
    let database_path = state.database_path;
    let result = tokio::task::spawn_blocking(move || {
        AlertStore::open(&database_path)?.list_alerts(page, page_size, sort, descending)
    })
    .await;

    match result {
        Ok(Ok(page)) => {
            let mut response = Json(page).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(response)
        }
        Ok(Err(error)) => Err(api_error(error.to_string())),
        Err(error) => Err(api_error(format!("alert query task failed: {error}"))),
    }
}

async fn alert_details(
    Path(candidate_id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let database_path = state.database_path;
    let result = tokio::task::spawn_blocking(move || {
        AlertStore::open(&database_path)?.get_alert(candidate_id)
    })
    .await;

    match result {
        Ok(Ok(alert)) => {
            let mut response = Json(alert).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(response)
        }
        Ok(Err(error)) => {
            let (status, message) = manual_analysis_error(error);
            Err(api_error_with_status(status, message))
        }
        Err(error) => Err(api_error(format!("alert detail task failed: {error}"))),
    }
}

async fn analyze_candidate(
    Path(candidate_id): Path<i64>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let Some(sender) = state.manual_analysis_sender else {
        return Err(api_error_with_status(
            StatusCode::SERVICE_UNAVAILABLE,
            "manual analysis is unavailable",
        ));
    };
    let database_path = state.database_path;
    let queued = tokio::task::spawn_blocking(move || {
        let store = AlertStore::open(&database_path).map_err(manual_analysis_error)?;
        let context = store
            .retry_candidate(candidate_id)
            .map_err(manual_analysis_error)?;
        match sender.try_send(ManualAnalysisRequest {
            candidate_id,
            context,
        }) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = match error {
                    TrySendError::Full(_) => "manual analysis queue is busy",
                    TrySendError::Disconnected(_) => "manual analysis worker is unavailable",
                };
                if let Err(rollback_error) = store.mark_candidate_failed(candidate_id, message) {
                    error!(
                        "Failed to restore candidate {candidate_id} after manual analysis enqueue failure: {rollback_error}"
                    );
                }
                Err((StatusCode::SERVICE_UNAVAILABLE, message.to_string()))
            }
        }
    })
    .await;

    match queued {
        Ok(Ok(())) => Ok((
            StatusCode::ACCEPTED,
            Json(AnalysisQueued {
                candidate_id,
                analysis_status: "pending",
            }),
        )
            .into_response()),
        Ok(Err((status, message))) => Err(api_error_with_status(status, message)),
        Err(error) => Err(api_error(format!("manual analysis task failed: {error}"))),
    }
}

fn manual_analysis_error(error: AlertError) -> (StatusCode, String) {
    let status = match error {
        AlertError::CandidateNotFound(_) => StatusCode::NOT_FOUND,
        AlertError::CandidateNotRetryable { .. } => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}

fn api_error(message: String) -> (StatusCode, Json<ApiError>) {
    api_error_with_status(StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn api_error_with_status(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<ApiError>) {
    let message = message.into();
    error!("Alert dashboard request failed: {message}");
    (status, Json(ApiError { message }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{LogEvent, MessageType};
    use crate::triggers::TriggerContext;
    use axum::body::to_bytes;
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn router_builds_with_a_database_path() {
        let _ = router(PathBuf::from("eyes.db"), None);
    }

    #[tokio::test]
    async fn dashboard_and_static_assets_are_not_cached() {
        let page_response = index().await;
        let script_response = script().await;

        assert_eq!(page_response.status(), StatusCode::OK);
        assert_eq!(page_response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(script_response.status(), StatusCode::OK);
        assert_eq!(script_response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            script_response.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        let script_body = to_bytes(script_response.into_body(), 1_000_000)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&script_body).contains("Analysis not done"));
    }

    #[tokio::test]
    async fn api_includes_unanalyzed_candidates() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut store = AlertStore::open(&database_path).unwrap();
        let mut context = TriggerContext::for_summary(&[], &[], &[]);
        context.triggered_by = "MemoryPressureRule".to_string();
        context.trigger_reason = "Memory pressure reached warning".to_string();
        context.log_events.push(LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Fault,
            subsystem: "com.example.editor".to_string(),
            category: "lifecycle".to_string(),
            process: "ExampleEditor".to_string(),
            process_id: 42,
            message: "Application crashed unexpectedly".to_string(),
        });
        let candidate_id = store.record_candidate(&context).unwrap();

        let response = alerts(
            State(AppState {
                database_path: database_path.clone(),
                manual_analysis_sender: None,
            }),
            Query(AlertQuery {
                page: Some(1),
                page_size: Some(10),
                sort: Some("time".to_string()),
                order: Some("desc".to_string()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");

        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["counts"]["total"], 1);
        assert_eq!(payload["alerts"][0]["analysis_status"], "pending");
        assert_eq!(
            payload["alerts"][0]["summary"],
            "Memory pressure reached warning"
        );
        assert!(payload["alerts"][0]["notification_title"].is_null());
        assert!(payload["alerts"][0].get("log_events").is_none());

        let detail_response = alert_details(
            Path(candidate_id),
            State(AppState {
                database_path,
                manual_analysis_sender: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(detail_response.status(), StatusCode::OK);
        assert_eq!(detail_response.headers()[header::CACHE_CONTROL], "no-store");
        let detail_body = to_bytes(detail_response.into_body(), 1_000_000)
            .await
            .unwrap();
        let detail: serde_json::Value = serde_json::from_slice(&detail_body).unwrap();
        assert_eq!(detail["log_events"][0]["process"], "ExampleEditor");
        assert_eq!(
            detail["log_events"][0]["message"],
            "Application crashed unexpectedly"
        );
    }

    #[tokio::test]
    async fn api_queues_not_done_candidate_for_manual_analysis() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut store = AlertStore::open(&database_path).unwrap();
        let mut context = TriggerContext::for_summary(&[], &[], &[]);
        context.triggered_by = "CrashDetectionRule".to_string();
        context.trigger_reason = "ExampleEditor crashed".to_string();
        let candidate_id = store.record_candidate(&context).unwrap();
        store
            .mark_candidate_not_done(candidate_id, "Automatic AI analysis is disabled")
            .unwrap();
        drop(store);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);

        let response = analyze_candidate(
            Path(candidate_id),
            State(AppState {
                database_path: database_path.clone(),
                manual_analysis_sender: Some(sender),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let request = receiver.recv().unwrap();
        assert_eq!(request.candidate_id, candidate_id);
        assert_eq!(request.context.triggered_by, "CrashDetectionRule");
        let page = AlertStore::open(&database_path)
            .unwrap()
            .list_alerts(1, 10, AlertSort::AssessedAt, true)
            .unwrap();
        assert_eq!(page.alerts[0].analysis_status, "pending");
    }
}
