use crate::alerts::{AlertStore, AutoGroupRuleInput};
use crate::error::AlertError;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAlertsParams {
    #[schemars(description = "Optional severity filter: info, warning, or critical")]
    pub severity: Option<String>,
    #[schemars(description = "Optional resolution filter: open or resolved")]
    pub resolution_status: Option<String>,
    #[schemars(description = "Maximum alerts to return, from 1 to 100; defaults to 25")]
    pub limit: Option<usize>,
    #[schemars(description = "Number of matching alerts to skip; defaults to 0")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchAlertsParams {
    #[schemars(
        description = "Text matched against summaries, root causes, trigger metadata, and agent reviews"
    )]
    pub query: String,
    #[schemars(description = "Optional severity filter: info, warning, or critical")]
    pub severity: Option<String>,
    #[schemars(description = "Optional resolution filter: open or resolved")]
    pub resolution_status: Option<String>,
    #[schemars(description = "Maximum alerts to return, from 1 to 100; defaults to 25")]
    pub limit: Option<usize>,
    #[schemars(description = "Number of matching alerts to skip; defaults to 0")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AlertIdParams {
    #[schemars(description = "Alert candidate ID")]
    pub alert_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveAlertParams {
    #[schemars(description = "Alert candidate ID")]
    pub alert_id: i64,
    #[schemars(description = "Name of the agent recording the resolution")]
    pub agent_name: String,
    #[schemars(description = "Resolution summary, including actions taken and outcome")]
    pub resolution: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AttachSimilarAlertsParams {
    #[schemars(description = "Root alert that will represent the group")]
    pub primary_alert_id: i64,
    #[schemars(description = "Alerts to fold under the root alert")]
    pub similar_alert_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AppendAgentReviewParams {
    #[schemars(description = "Alert candidate ID")]
    pub alert_id: i64,
    #[schemars(description = "Name of the agent writing the review")]
    pub agent_name: String,
    #[schemars(description = "Review text to append to the alert history")]
    pub review: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateAutoGroupRuleParams {
    #[schemars(description = "Existing alert that will become the group's canonical root")]
    pub target_alert_id: i64,
    #[schemars(description = "Optional exact, case-sensitive log process name")]
    pub process: Option<String>,
    #[schemars(description = "Optional exact, case-sensitive log subsystem")]
    pub subsystem: Option<String>,
    #[schemars(description = "Optional exact, case-sensitive trigger source")]
    pub trigger_source: Option<String>,
    #[schemars(description = "Optional exact, case-sensitive trigger rule name")]
    pub triggered_by: Option<String>,
    #[schemars(description = "Rust regular expression matched against a log event message")]
    pub message_regex: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteAutoGroupRuleParams {
    #[schemars(description = "Auto-group rule ID")]
    pub rule_id: i64,
}

#[derive(Clone)]
pub struct AlertMcpServer {
    database_path: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl AlertMcpServer {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            database_path,
            tool_router: Self::tool_router(),
        }
    }

    fn open_store(&self) -> Result<AlertStore, AlertError> {
        AlertStore::open(&self.database_path)
    }
}

#[tool_router]
impl AlertMcpServer {
    #[tool(
        description = "List alert summaries, optionally filtered by severity or resolution state"
    )]
    fn list_alerts(
        &self,
        Parameters(params): Parameters<ListAlertsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|store| {
            store.search_alerts(
                None,
                params.severity.as_deref(),
                params.resolution_status.as_deref(),
                params.limit.unwrap_or(25),
                params.offset.unwrap_or(0),
            )
        })))
    }

    #[tool(
        description = "Search alerts by text across diagnoses, trigger metadata, and agent reviews"
    )]
    fn search_alerts(
        &self,
        Parameters(params): Parameters<SearchAlertsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if params.query.trim().is_empty() {
            return Ok(tool_error("query cannot be empty"));
        }
        Ok(tool_result(self.open_store().and_then(|store| {
            store.search_alerts(
                Some(&params.query),
                params.severity.as_deref(),
                params.resolution_status.as_deref(),
                params.limit.unwrap_or(25),
                params.offset.unwrap_or(0),
            )
        })))
    }

    #[tool(
        description = "Get complete persisted data for one alert, including raw evidence, reviews, and grouped alerts"
    )]
    fn get_alert(
        &self,
        Parameters(params): Parameters<AlertIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(
            self.open_store()
                .and_then(|store| store.get_alert(params.alert_id)),
        ))
    }

    #[tool(
        description = "Resolve an open alert and atomically append the agent's resolution to its history"
    )]
    fn resolve_alert(
        &self,
        Parameters(params): Parameters<ResolveAlertParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|mut store| {
            store.resolve_alert(params.alert_id, &params.agent_name, &params.resolution)
        })))
    }

    #[tool(
        description = "Attach similar alerts beneath one root alert; existing child groups are merged into the root"
    )]
    fn attach_similar_alerts(
        &self,
        Parameters(params): Parameters<AttachSimilarAlertsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|mut store| {
            store.attach_similar_alerts(params.primary_alert_id, &params.similar_alert_ids)
        })))
    }

    #[tool(description = "Append an agent-authored review to an alert without resolving it")]
    fn append_agent_review(
        &self,
        Parameters(params): Parameters<AppendAgentReviewParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|store| {
            store.append_agent_review(params.alert_id, &params.agent_name, &params.review)
        })))
    }

    #[tool(
        description = "Create a deterministic rule that folds future matching alerts under an existing alert. A message regex must be paired with at least one exact process, subsystem, trigger source, or trigger rule selector. The first-created matching rule wins."
    )]
    fn create_auto_group_rule(
        &self,
        Parameters(params): Parameters<CreateAutoGroupRuleParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|mut store| {
            store.create_auto_group_rule(AutoGroupRuleInput {
                target_alert_id: params.target_alert_id,
                process: params.process,
                subsystem: params.subsystem,
                trigger_source: params.trigger_source,
                triggered_by: params.triggered_by,
                message_regex: params.message_regex,
            })
        })))
    }

    #[tool(description = "List auto-group rules in matching precedence order")]
    fn list_auto_group_rules(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(
            self.open_store()
                .and_then(|store| store.list_auto_group_rules()),
        ))
    }

    #[tool(description = "Delete an auto-group rule so it no longer affects future alerts")]
    fn delete_auto_group_rule(
        &self,
        Parameters(params): Parameters<DeleteAutoGroupRuleParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(tool_result(self.open_store().and_then(|mut store| {
            store.delete_auto_group_rule(params.rule_id)
        })))
    }
}

#[tool_handler]
impl ServerHandler for AlertMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Inspect and triage alerts captured by Eyes. Read complete alert evidence before resolving, grouping alerts, or creating future auto-group rules."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "eyes-alerts".to_string(),
                title: Some("Eyes Alert Triage".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }
}

pub async fn serve(database_path: PathBuf) -> anyhow::Result<()> {
    AlertStore::open(&database_path)?;
    let service = AlertMcpServer::new(database_path)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

fn tool_result<T: Serialize>(result: Result<T, AlertError>) -> CallToolResult {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => tool_error(format!("failed to serialize tool result: {error}")),
        },
        Err(error) => tool_error(error.to_string()),
    }
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({ "error": message.into() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_is_structured_and_visible_to_callers() {
        let result = tool_error("bad alert");

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.structured_content,
            Some(json!({ "error": "bad alert" }))
        );
    }

    #[test]
    fn list_result_preserves_pagination_metadata() {
        let result =
            tool_result::<crate::alerts::AlertSearchPage>(Ok(crate::alerts::AlertSearchPage {
                alerts: Vec::new(),
                total: 0,
                limit: 25,
                offset: 0,
            }));

        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content.unwrap()["limit"], 25);
    }
}
