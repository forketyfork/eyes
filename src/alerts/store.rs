use crate::ai::AIInsight;
use crate::error::AlertError;
use crate::events::{DiskEvent, LogEvent, MetricsEvent, Severity};
use crate::triggers::TriggerContext;
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const SCHEMA_VERSION: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertStatus {
    Pending,
    Suppressed,
    Queued,
    Delivered,
    DeliveryFailed,
    Dropped,
}

impl AlertStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Suppressed => "suppressed",
            Self::Queued => "queued",
            Self::Delivered => "delivered",
            Self::DeliveryFailed => "delivery_failed",
            Self::Dropped => "dropped",
        }
    }
}

#[derive(Debug)]
pub struct AlertStore {
    connection: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSort {
    AssessedAt,
    Severity,
    Status,
    Summary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AlertRecord {
    pub id: i64,
    pub assessed_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub summary: String,
    pub root_cause: Option<String>,
    pub severity: String,
    pub observation_confidence: String,
    pub diagnosis_confidence: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
    pub notification_title: Option<String>,
    pub notification_body: Option<String>,
    pub status: Option<String>,
    pub delivery_attempted_at: Option<String>,
    pub delivered_at: Option<String>,
    pub failure_message: Option<String>,
    pub analysis_status: String,
    pub analysis_failure: Option<String>,
    pub triggered_by: String,
    pub trigger_source: Option<String>,
    pub trigger_reason: String,
    pub log_event_count: usize,
    pub metrics_event_count: usize,
    pub disk_event_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub log_events: Vec<LogEvent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub metrics_events: Vec<MetricsEvent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disk_events: Vec<DiskEvent>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct AlertCounts {
    pub total: usize,
    pub critical: usize,
    pub warning: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AlertPage {
    pub alerts: Vec<AlertRecord>,
    pub counts: AlertCounts,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
}

const ALERT_COLUMNS: &str = "c.id, c.triggered_at, c.triggered_at, c.updated_at,
     COALESCE(s.summary, c.trigger_reason), s.root_cause,
     COALESCE(s.severity, c.expected_severity),
     COALESCE(s.observation_confidence, 'not analyzed'),
     COALESCE(s.diagnosis_confidence, 'not analyzed'),
     a.notification_title, a.notification_body, a.status,
     a.delivery_attempted_at, a.delivered_at, a.failure_message,
     c.analysis_status, c.analysis_failure, c.trigger_rule,
     c.trigger_source, c.trigger_reason, c.log_event_count,
     c.metrics_event_count, c.disk_event_count";

fn alert_record_from_row(row: &Row<'_>) -> rusqlite::Result<AlertRecord> {
    Ok(AlertRecord {
        id: row.get(0)?,
        assessed_at: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        summary: row.get(4)?,
        root_cause: row.get(5)?,
        severity: row.get(6)?,
        observation_confidence: row.get(7)?,
        diagnosis_confidence: row.get(8)?,
        recommendations: Vec::new(),
        evidence: Vec::new(),
        limitations: Vec::new(),
        notification_title: row.get(9)?,
        notification_body: row.get(10)?,
        status: row.get(11)?,
        delivery_attempted_at: row.get(12)?,
        delivered_at: row.get(13)?,
        failure_message: row.get(14)?,
        analysis_status: row.get(15)?,
        analysis_failure: row.get(16)?,
        triggered_by: row.get(17)?,
        trigger_source: row.get(18)?,
        trigger_reason: row.get(19)?,
        log_event_count: row.get::<_, i64>(20)? as usize,
        metrics_event_count: row.get::<_, i64>(21)? as usize,
        disk_event_count: row.get::<_, i64>(22)? as usize,
        log_events: Vec::new(),
        metrics_events: Vec::new(),
        disk_events: Vec::new(),
    })
}

impl AlertStore {
    pub fn open(path: &Path) -> Result<Self, AlertError> {
        let connection = Connection::open(path).map_err(|error| {
            AlertError::PersistenceFailed(format!(
                "failed to open database '{}': {error}",
                path.display()
            ))
        })?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(persistence_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(persistence_error)?;

        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn record_alert(
        &mut self,
        insight: &AIInsight,
        notification_title: &str,
        notification_body: &str,
        status: AlertStatus,
    ) -> Result<i64, AlertError> {
        self.record_alert_for_candidate(
            None,
            insight,
            notification_title,
            notification_body,
            status,
        )
    }

    pub fn record_alert_for_candidate(
        &mut self,
        candidate_id: Option<i64>,
        insight: &AIInsight,
        notification_title: &str,
        notification_body: &str,
        status: AlertStatus,
    ) -> Result<i64, AlertError> {
        let transaction = self.connection.transaction().map_err(persistence_error)?;
        transaction
            .execute(
                "INSERT INTO assessments (
                    assessed_at, summary, root_cause, severity,
                    observation_confidence, diagnosis_confidence
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format_timestamp(insight.timestamp),
                    insight.summary,
                    insight.root_cause,
                    severity_name(insight),
                    insight.observation_confidence,
                    insight.diagnosis_confidence,
                ],
            )
            .map_err(persistence_error)?;
        let assessment_id = transaction.last_insert_rowid();

        insert_ordered_values(
            &transaction,
            "assessment_recommendations",
            "recommendation",
            assessment_id,
            &insight.recommendations,
        )?;
        insert_ordered_values(
            &transaction,
            "assessment_evidence",
            "evidence",
            assessment_id,
            &insight.evidence,
        )?;
        insert_ordered_values(
            &transaction,
            "assessment_limitations",
            "limitation",
            assessment_id,
            &insight.limitations,
        )?;

        let now = current_timestamp();
        transaction
            .execute(
                "INSERT INTO alerts (
                    assessment_id, created_at, updated_at, notification_title,
                    notification_body, status
                ) VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
                params![
                    assessment_id,
                    now,
                    notification_title,
                    notification_body,
                    status.as_str(),
                ],
            )
            .map_err(persistence_error)?;
        let alert_id = transaction.last_insert_rowid();
        match candidate_id {
            Some(candidate_id) => {
                let updated = transaction
                    .execute(
                        "UPDATE alert_candidates SET
                            updated_at = ?1,
                            analysis_status = 'analyzed',
                            analysis_failure = NULL,
                            assessment_id = ?2,
                            alert_id = ?3
                         WHERE id = ?4",
                        params![now, assessment_id, alert_id, candidate_id],
                    )
                    .map_err(persistence_error)?;
                if updated != 1 {
                    return Err(AlertError::PersistenceFailed(format!(
                        "alert candidate {candidate_id} does not exist"
                    )));
                }
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO alert_candidates (
                            triggered_at, updated_at, trigger_rule, trigger_reason,
                            expected_severity, analysis_status, assessment_id, alert_id,
                            log_event_count, metrics_event_count, disk_event_count
                         ) VALUES (?1, ?1, 'direct_alert', ?2, ?3, 'analyzed', ?4, ?5, 0, 0, 0)",
                        params![
                            format_timestamp(insight.timestamp),
                            insight.summary,
                            severity_value(insight.severity),
                            assessment_id,
                            alert_id,
                        ],
                    )
                    .map_err(persistence_error)?;
            }
        }
        transaction.commit().map_err(persistence_error)?;
        Ok(alert_id)
    }

    pub fn record_candidate(&mut self, context: &TriggerContext) -> Result<i64, AlertError> {
        let timestamp = format_timestamp(context.timestamp);
        let transaction = self.connection.transaction().map_err(persistence_error)?;
        transaction
            .execute(
                "INSERT INTO alert_candidates (
                    triggered_at, updated_at, trigger_rule, trigger_source, trigger_reason,
                    expected_severity, analysis_status, log_event_count,
                    metrics_event_count, disk_event_count
                 ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8)",
                params![
                    timestamp,
                    context.triggered_by,
                    context.trigger_source,
                    context.trigger_reason,
                    severity_value(context.expected_severity),
                    context.log_events.len() as i64,
                    context.metrics_events.len() as i64,
                    context.disk_events.len() as i64,
                ],
            )
            .map_err(persistence_error)?;
        let candidate_id = transaction.last_insert_rowid();
        insert_context_events(&transaction, candidate_id, "log", &context.log_events)?;
        insert_context_events(
            &transaction,
            candidate_id,
            "metrics",
            &context.metrics_events,
        )?;
        insert_context_events(&transaction, candidate_id, "disk", &context.disk_events)?;
        transaction.commit().map_err(persistence_error)?;
        Ok(candidate_id)
    }

    pub fn retry_candidate(&self, candidate_id: i64) -> Result<TriggerContext, AlertError> {
        let candidate = self
            .connection
            .query_row(
                "SELECT triggered_at, trigger_rule, trigger_source, trigger_reason,
                        expected_severity, analysis_status
                 FROM alert_candidates
                 WHERE id = ?1",
                [candidate_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(persistence_error)?;
        let Some((timestamp, triggered_by, trigger_source, trigger_reason, severity, status)) =
            candidate
        else {
            return Err(AlertError::CandidateNotFound(candidate_id));
        };
        if status != "failed" {
            return Err(AlertError::CandidateNotRetryable {
                candidate_id,
                status,
            });
        }

        let context = TriggerContext {
            timestamp: timestamp.parse().map_err(|error| {
                AlertError::PersistenceFailed(format!(
                    "invalid trigger timestamp for alert candidate {candidate_id}: {error}"
                ))
            })?,
            log_events: self.context_events(candidate_id, "log")?,
            metrics_events: self.context_events(candidate_id, "metrics")?,
            disk_events: self.context_events(candidate_id, "disk")?,
            triggered_by,
            trigger_source,
            expected_severity: parse_severity(&severity, candidate_id)?,
            trigger_reason,
        };
        let updated = self
            .connection
            .execute(
                "UPDATE alert_candidates SET
                    updated_at = ?1,
                    analysis_status = 'pending',
                    analysis_failure = NULL
                 WHERE id = ?2 AND analysis_status = 'failed'",
                params![current_timestamp(), candidate_id],
            )
            .map_err(persistence_error)?;
        if updated != 1 {
            return Err(AlertError::CandidateNotRetryable {
                candidate_id,
                status: "changed concurrently".to_string(),
            });
        }
        Ok(context)
    }

    pub fn mark_candidate_failed(
        &self,
        candidate_id: i64,
        failure_message: &str,
    ) -> Result<(), AlertError> {
        let updated = self
            .connection
            .execute(
                "UPDATE alert_candidates SET
                    updated_at = ?1,
                    analysis_status = 'failed',
                    analysis_failure = ?2
                 WHERE id = ?3 AND analysis_status = 'pending'",
                params![current_timestamp(), failure_message, candidate_id],
            )
            .map_err(persistence_error)?;
        if updated != 1 {
            return Err(AlertError::PersistenceFailed(format!(
                "pending alert candidate {candidate_id} does not exist"
            )));
        }
        Ok(())
    }

    pub fn fail_pending_candidates(&self, failure_message: &str) -> Result<usize, AlertError> {
        self.connection
            .execute(
                "UPDATE alert_candidates SET
                    updated_at = ?1,
                    analysis_status = 'failed',
                    analysis_failure = ?2
                 WHERE analysis_status = 'pending'",
                params![current_timestamp(), failure_message],
            )
            .map_err(persistence_error)
    }

    pub fn update_status(
        &self,
        alert_id: i64,
        status: AlertStatus,
        failure_message: Option<&str>,
    ) -> Result<(), AlertError> {
        let now = current_timestamp();
        let updated = self
            .connection
            .execute(
                "UPDATE alerts SET
                    status = ?1,
                    updated_at = ?2,
                    delivery_attempted_at = CASE
                        WHEN ?1 IN ('delivered', 'delivery_failed')
                        THEN COALESCE(delivery_attempted_at, ?2)
                        ELSE delivery_attempted_at
                    END,
                    delivered_at = CASE WHEN ?1 = 'delivered' THEN ?2 ELSE delivered_at END,
                    failure_message = ?3
                 WHERE id = ?4",
                params![status.as_str(), now, failure_message, alert_id],
            )
            .map_err(persistence_error)?;

        if updated != 1 {
            return Err(AlertError::PersistenceFailed(format!(
                "alert {alert_id} does not exist"
            )));
        }
        Ok(())
    }

    pub fn list_alerts(
        &self,
        page: usize,
        page_size: usize,
        sort: AlertSort,
        descending: bool,
    ) -> Result<AlertPage, AlertError> {
        let page = page.max(1);
        let page_size = page_size.clamp(5, 50);
        let counts = self.alert_counts()?;
        let total_pages = counts.total.div_ceil(page_size);
        let offset = (page - 1).saturating_mul(page_size).min(i64::MAX as usize) as i64;
        let sort_column = match sort {
            AlertSort::AssessedAt => "c.triggered_at",
            AlertSort::Severity => {
                "CASE COALESCE(s.severity, c.expected_severity)
                    WHEN 'critical' THEN 3 WHEN 'warning' THEN 2 ELSE 1 END"
            }
            AlertSort::Status => "c.analysis_status",
            AlertSort::Summary => "COALESCE(s.summary, c.trigger_reason) COLLATE NOCASE",
        };
        let direction = if descending { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT {ALERT_COLUMNS}
             FROM alert_candidates c
             LEFT JOIN assessments s ON s.id = c.assessment_id
             LEFT JOIN alerts a ON a.id = c.alert_id
             ORDER BY {sort_column} {direction}, c.id {direction}
             LIMIT ?1 OFFSET ?2"
        );
        let mut statement = self.connection.prepare(&sql).map_err(persistence_error)?;
        let rows = statement
            .query_map(params![page_size as i64, offset], alert_record_from_row)
            .map_err(persistence_error)?;
        let alerts = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence_error)?;

        Ok(AlertPage {
            alerts,
            counts,
            page,
            page_size,
            total_pages,
        })
    }

    pub fn get_alert(&self, candidate_id: i64) -> Result<AlertRecord, AlertError> {
        let sql = format!(
            "SELECT {ALERT_COLUMNS}
             FROM alert_candidates c
             LEFT JOIN assessments s ON s.id = c.assessment_id
             LEFT JOIN alerts a ON a.id = c.alert_id
             WHERE c.id = ?1"
        );
        let mut alert = self
            .connection
            .query_row(&sql, [candidate_id], alert_record_from_row)
            .optional()
            .map_err(persistence_error)?
            .ok_or(AlertError::CandidateNotFound(candidate_id))?;

        alert.recommendations = self.ordered_assessment_values(
            alert.id,
            "assessment_recommendations",
            "recommendation",
        )?;
        alert.evidence =
            self.ordered_assessment_values(alert.id, "assessment_evidence", "evidence")?;
        alert.limitations =
            self.ordered_assessment_values(alert.id, "assessment_limitations", "limitation")?;
        alert.log_events = self.context_events(alert.id, "log")?;
        alert.metrics_events = self.context_events(alert.id, "metrics")?;
        alert.disk_events = self.context_events(alert.id, "disk")?;

        Ok(alert)
    }

    #[cfg(test)]
    pub(crate) fn execute_batch_for_testing(&self, sql: &str) -> Result<(), AlertError> {
        self.connection
            .execute_batch(sql)
            .map_err(persistence_error)
    }

    fn migrate(&mut self) -> Result<(), AlertError> {
        let mut version = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            .map_err(persistence_error)?;

        if version > SCHEMA_VERSION {
            return Err(AlertError::PersistenceFailed(format!(
                "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            )));
        }

        if version == 0 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(persistence_error)?;
            transaction
                .execute_batch(
                    "CREATE TABLE assessments (
                         id INTEGER PRIMARY KEY,
                         assessed_at TEXT NOT NULL,
                         summary TEXT NOT NULL,
                         root_cause TEXT,
                         severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
                         observation_confidence TEXT NOT NULL,
                         diagnosis_confidence TEXT NOT NULL
                     );
                     CREATE TABLE assessment_recommendations (
                         assessment_id INTEGER NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
                         position INTEGER NOT NULL CHECK (position >= 0),
                         recommendation TEXT NOT NULL,
                         PRIMARY KEY (assessment_id, position)
                     );
                     CREATE TABLE assessment_evidence (
                         assessment_id INTEGER NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
                         position INTEGER NOT NULL CHECK (position >= 0),
                         evidence TEXT NOT NULL,
                         PRIMARY KEY (assessment_id, position)
                     );
                     CREATE TABLE assessment_limitations (
                         assessment_id INTEGER NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
                         position INTEGER NOT NULL CHECK (position >= 0),
                         limitation TEXT NOT NULL,
                         PRIMARY KEY (assessment_id, position)
                     );
                     CREATE TABLE alerts (
                         id INTEGER PRIMARY KEY,
                         assessment_id INTEGER NOT NULL UNIQUE REFERENCES assessments(id) ON DELETE CASCADE,
                         created_at TEXT NOT NULL,
                         updated_at TEXT NOT NULL,
                         notification_title TEXT NOT NULL,
                         notification_body TEXT NOT NULL,
                         status TEXT NOT NULL CHECK (
                             status IN ('pending', 'suppressed', 'queued', 'delivered', 'delivery_failed', 'dropped')
                         ),
                         delivery_attempted_at TEXT,
                         delivered_at TEXT,
                         failure_message TEXT
                     );
                     CREATE INDEX alerts_status_created_at_idx ON alerts(status, created_at);
                     CREATE INDEX assessments_severity_assessed_at_idx
                         ON assessments(severity, assessed_at);
                     PRAGMA user_version = 1;",
                )
                .map_err(persistence_error)?;
            transaction.commit().map_err(persistence_error)?;
            version = 1;
        }

        if version == 1 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(persistence_error)?;
            transaction
                .execute_batch(
                    "CREATE TABLE alert_candidates (
                         id INTEGER PRIMARY KEY,
                         triggered_at TEXT NOT NULL,
                         updated_at TEXT NOT NULL,
                         trigger_rule TEXT NOT NULL,
                         trigger_source TEXT,
                         trigger_reason TEXT NOT NULL,
                         expected_severity TEXT NOT NULL CHECK (
                             expected_severity IN ('info', 'warning', 'critical')
                         ),
                         analysis_status TEXT NOT NULL CHECK (
                             analysis_status IN ('pending', 'analyzed', 'failed')
                         ),
                         analysis_failure TEXT,
                         assessment_id INTEGER UNIQUE REFERENCES assessments(id) ON DELETE SET NULL,
                         alert_id INTEGER UNIQUE REFERENCES alerts(id) ON DELETE SET NULL,
                         log_event_count INTEGER NOT NULL CHECK (log_event_count >= 0),
                         metrics_event_count INTEGER NOT NULL CHECK (metrics_event_count >= 0),
                         disk_event_count INTEGER NOT NULL CHECK (disk_event_count >= 0)
                     );
                     INSERT INTO alert_candidates (
                         triggered_at, updated_at, trigger_rule, trigger_reason,
                         expected_severity, analysis_status, assessment_id, alert_id,
                         log_event_count, metrics_event_count, disk_event_count
                     )
                     SELECT s.assessed_at, a.updated_at, 'legacy_alert',
                            'Analyzed before trigger candidate tracking', s.severity,
                            'analyzed', s.id, a.id, 0, 0, 0
                     FROM alerts a
                     JOIN assessments s ON s.id = a.assessment_id;
                     CREATE INDEX alert_candidates_status_triggered_at_idx
                         ON alert_candidates(analysis_status, triggered_at);
                     CREATE INDEX alert_candidates_severity_triggered_at_idx
                         ON alert_candidates(expected_severity, triggered_at);
                     PRAGMA user_version = 2;",
                )
                .map_err(persistence_error)?;
            transaction.commit().map_err(persistence_error)?;
            version = 2;
        }

        if version == 2 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(persistence_error)?;
            transaction
                .execute_batch(
                    "CREATE TABLE alert_candidate_context_events (
                         candidate_id INTEGER NOT NULL
                             REFERENCES alert_candidates(id) ON DELETE CASCADE,
                         event_kind TEXT NOT NULL CHECK (
                             event_kind IN ('log', 'metrics', 'disk')
                         ),
                         position INTEGER NOT NULL CHECK (position >= 0),
                         payload TEXT NOT NULL,
                         PRIMARY KEY (candidate_id, event_kind, position)
                     );
                     PRAGMA user_version = 3;",
                )
                .map_err(persistence_error)?;
            transaction.commit().map_err(persistence_error)?;
        }

        Ok(())
    }

    fn alert_counts(&self) -> Result<AlertCounts, AlertError> {
        let mut counts = AlertCounts::default();
        let mut statement = self
            .connection
            .prepare(
                "SELECT COALESCE(s.severity, c.expected_severity), COUNT(*)
                 FROM alert_candidates c
                 LEFT JOIN assessments s ON s.id = c.assessment_id
                 GROUP BY COALESCE(s.severity, c.expected_severity)",
            )
            .map_err(persistence_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(persistence_error)?;

        for row in rows {
            let (severity, count) = row.map_err(persistence_error)?;
            let count = count as usize;
            counts.total += count;
            match severity.as_str() {
                "critical" => counts.critical = count,
                "warning" => counts.warning = count,
                "info" => counts.info = count,
                _ => {}
            }
        }
        Ok(counts)
    }

    fn ordered_assessment_values(
        &self,
        alert_id: i64,
        table: &str,
        value_column: &str,
    ) -> Result<Vec<String>, AlertError> {
        let sql = format!(
            "SELECT v.{value_column}
             FROM {table} v
             JOIN alert_candidates c ON c.assessment_id = v.assessment_id
             WHERE c.id = ?1
             ORDER BY v.position"
        );
        let mut statement = self
            .connection
            .prepare_cached(&sql)
            .map_err(persistence_error)?;
        let values = statement
            .query_map([alert_id], |row| row.get(0))
            .map_err(persistence_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence_error)?;
        Ok(values)
    }

    fn context_events<T: DeserializeOwned>(
        &self,
        candidate_id: i64,
        event_kind: &str,
    ) -> Result<Vec<T>, AlertError> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT payload
                 FROM alert_candidate_context_events
                 WHERE candidate_id = ?1 AND event_kind = ?2
                 ORDER BY position",
            )
            .map_err(persistence_error)?;
        let payloads = statement
            .query_map(params![candidate_id, event_kind], |row| {
                row.get::<_, String>(0)
            })
            .map_err(persistence_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence_error)?;
        payloads
            .into_iter()
            .map(|payload| {
                serde_json::from_str(&payload).map_err(|error| {
                    AlertError::PersistenceFailed(format!(
                        "invalid {event_kind} context for alert candidate {candidate_id}: {error}"
                    ))
                })
            })
            .collect()
    }
}

fn insert_context_events<T: Serialize>(
    connection: &Connection,
    candidate_id: i64,
    event_kind: &str,
    events: &[T],
) -> Result<(), AlertError> {
    let mut statement = connection
        .prepare_cached(
            "INSERT INTO alert_candidate_context_events (
                candidate_id, event_kind, position, payload
             ) VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(persistence_error)?;
    for (position, event) in events.iter().enumerate() {
        let payload = serde_json::to_string(event).map_err(|error| {
            AlertError::PersistenceFailed(format!(
                "failed to serialize {event_kind} trigger context: {error}"
            ))
        })?;
        statement
            .execute(params![candidate_id, event_kind, position as i64, payload])
            .map_err(persistence_error)?;
    }
    Ok(())
}

fn insert_ordered_values(
    connection: &Connection,
    table: &str,
    value_column: &str,
    assessment_id: i64,
    values: &[String],
) -> Result<(), AlertError> {
    let sql = format!(
        "INSERT INTO {table} (assessment_id, position, {value_column}) VALUES (?1, ?2, ?3)"
    );
    let mut statement = connection.prepare_cached(&sql).map_err(persistence_error)?;
    for (position, value) in values.iter().enumerate() {
        statement
            .execute(params![assessment_id, position as i64, value])
            .map_err(persistence_error)?;
    }
    Ok(())
}

fn severity_name(insight: &AIInsight) -> &'static str {
    severity_value(insight.severity)
}

fn severity_value(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    }
}

fn parse_severity(value: &str, candidate_id: i64) -> Result<Severity, AlertError> {
    match value {
        "info" => Ok(Severity::Info),
        "warning" => Ok(Severity::Warning),
        "critical" => Ok(Severity::Critical),
        _ => Err(AlertError::PersistenceFailed(format!(
            "invalid severity '{value}' for alert candidate {candidate_id}"
        ))),
    }
}

fn current_timestamp() -> String {
    format_timestamp(Utc::now())
}

fn format_timestamp(timestamp: chrono::DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn persistence_error(error: rusqlite::Error) -> AlertError {
    AlertError::PersistenceFailed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{MessageType, Severity};
    use tempfile::tempdir;

    fn test_insight() -> AIInsight {
        AIInsight {
            timestamp: Utc::now(),
            summary: "Memory pressure".to_string(),
            root_cause: Some("Large working set".to_string()),
            recommendations: vec![
                "Close unused applications".to_string(),
                "Add memory".to_string(),
            ],
            evidence: vec!["Pressure is critical".to_string()],
            observation_confidence: "high".to_string(),
            diagnosis_confidence: "medium".to_string(),
            limitations: vec!["No historical baseline".to_string()],
            severity: Severity::Critical,
        }
    }

    #[test]
    fn records_structured_assessment_and_alert() {
        let directory = tempdir().unwrap();
        let mut store = AlertStore::open(&directory.path().join("alerts.db")).unwrap();
        let insight = test_insight();

        let alert_id = store
            .record_alert(
                &insight,
                "System Alert",
                "Close unused applications",
                AlertStatus::Queued,
            )
            .unwrap();

        let alert: (String, String, String) = store
            .connection
            .query_row(
                "SELECT a.status, s.summary, s.severity
                 FROM alerts a JOIN assessments s ON s.id = a.assessment_id
                 WHERE a.id = ?1",
                [alert_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            alert,
            (
                "queued".to_string(),
                insight.summary,
                "critical".to_string()
            )
        );

        let recommendations: Vec<String> = store
            .connection
            .prepare(
                "SELECT recommendation FROM assessment_recommendations
                 ORDER BY position",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(recommendations, insight.recommendations);
    }

    #[test]
    fn updates_delivery_lifecycle() {
        let directory = tempdir().unwrap();
        let mut store = AlertStore::open(&directory.path().join("alerts.db")).unwrap();
        let alert_id = store
            .record_alert(&test_insight(), "System Alert", "Body", AlertStatus::Queued)
            .unwrap();

        store
            .update_status(
                alert_id,
                AlertStatus::DeliveryFailed,
                Some("permission denied"),
            )
            .unwrap();

        let state: (String, Option<String>, Option<String>) = store
            .connection
            .query_row(
                "SELECT status, delivery_attempted_at, failure_message FROM alerts WHERE id = ?1",
                [alert_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(state.0, "delivery_failed");
        assert!(state.1.is_some());
        assert_eq!(state.2.as_deref(), Some("permission denied"));
    }

    #[test]
    fn failed_migration_rolls_back_schema_changes() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch("CREATE TABLE assessment_recommendations (marker INTEGER);")
            .unwrap();
        drop(connection);

        assert!(AlertStore::open(&database_path).is_err());

        let connection = Connection::open(database_path).unwrap();
        let assessments_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'assessments'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assessments_table_count, 0);
        connection
            .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
            .unwrap();
    }

    #[test]
    fn lists_alert_summaries_and_loads_details_separately() {
        let directory = tempdir().unwrap();
        let mut store = AlertStore::open(&directory.path().join("alerts.db")).unwrap();

        for index in 0..6 {
            let mut insight = test_insight();
            insight.summary = format!("Assessment {index}");
            insight.severity = match index % 3 {
                0 => Severity::Info,
                1 => Severity::Warning,
                _ => Severity::Critical,
            };
            store
                .record_alert(
                    &insight,
                    "System Alert",
                    "Notification body",
                    AlertStatus::Delivered,
                )
                .unwrap();
        }

        let first_page = store
            .list_alerts(1, 5, AlertSort::AssessedAt, true)
            .unwrap();
        assert_eq!(first_page.alerts.len(), 5);
        assert_eq!(first_page.alerts[0].summary, "Assessment 5");
        assert!(first_page.alerts[0].recommendations.is_empty());
        assert_eq!(first_page.counts.total, 6);
        assert_eq!(first_page.counts.critical, 2);
        assert_eq!(first_page.counts.warning, 2);
        assert_eq!(first_page.counts.info, 2);
        assert_eq!(first_page.total_pages, 2);

        let second_page = store
            .list_alerts(2, 5, AlertSort::AssessedAt, true)
            .unwrap();
        assert_eq!(second_page.alerts.len(), 1);
        assert_eq!(second_page.alerts[0].summary, "Assessment 0");

        let severity_order = store.list_alerts(1, 5, AlertSort::Severity, false).unwrap();
        assert_eq!(severity_order.alerts[0].severity, "info");

        let details = store.get_alert(first_page.alerts[0].id).unwrap();
        assert_eq!(details.recommendations.len(), 2);
        assert_eq!(details.evidence.len(), 1);
    }

    #[test]
    fn lists_pending_failed_and_analyzed_candidates() {
        let directory = tempdir().unwrap();
        let mut store = AlertStore::open(&directory.path().join("alerts.db")).unwrap();
        let mut pending_context = TriggerContext::for_summary(&[], &[], &[]);
        pending_context.triggered_by = "MemoryPressureRule".to_string();
        pending_context.trigger_source = Some("system memory".to_string());
        pending_context.trigger_reason = "Memory pressure reached warning".to_string();
        pending_context.expected_severity = Severity::Warning;
        pending_context.log_events.push(LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Fault,
            subsystem: "com.example.editor".to_string(),
            category: "lifecycle".to_string(),
            process: "ExampleEditor".to_string(),
            process_id: 42,
            message: "Application crashed unexpectedly".to_string(),
        });

        let pending_id = store.record_candidate(&pending_context).unwrap();
        let mut failed_context = pending_context.clone();
        failed_context.triggered_by = "DiskIOSpikeRule".to_string();
        failed_context.trigger_reason = "Disk throughput spiked".to_string();
        let failed_id = store.record_candidate(&failed_context).unwrap();
        store
            .mark_candidate_failed(failed_id, "backend unavailable")
            .unwrap();

        let mut analyzed_context = pending_context.clone();
        analyzed_context.triggered_by = "ErrorFrequencyRule".to_string();
        analyzed_context.trigger_reason = "Errors repeated".to_string();
        let analyzed_id = store.record_candidate(&analyzed_context).unwrap();
        store
            .record_alert_for_candidate(
                Some(analyzed_id),
                &test_insight(),
                "System Alert",
                "Notification body",
                AlertStatus::Delivered,
            )
            .unwrap();

        let page = store
            .list_alerts(1, 10, AlertSort::AssessedAt, false)
            .unwrap();
        assert_eq!(page.counts.total, 3);

        let pending = page
            .alerts
            .iter()
            .find(|candidate| candidate.id == pending_id)
            .unwrap();
        assert_eq!(pending.analysis_status, "pending");
        assert_eq!(pending.summary, "Memory pressure reached warning");
        assert!(pending.notification_title.is_none());
        assert!(pending.log_events.is_empty());

        let pending_details = store.get_alert(pending_id).unwrap();
        assert_eq!(pending_details.log_events.len(), 1);
        assert_eq!(pending_details.log_events[0].process, "ExampleEditor");
        assert_eq!(
            pending_details.log_events[0].message,
            "Application crashed unexpectedly"
        );

        let failed = page
            .alerts
            .iter()
            .find(|candidate| candidate.id == failed_id)
            .unwrap();
        assert_eq!(failed.analysis_status, "failed");
        assert_eq!(
            failed.analysis_failure.as_deref(),
            Some("backend unavailable")
        );

        let analyzed = page
            .alerts
            .iter()
            .find(|candidate| candidate.id == analyzed_id)
            .unwrap();
        assert_eq!(analyzed.analysis_status, "analyzed");
        assert_eq!(analyzed.summary, "Memory pressure");
        assert_eq!(analyzed.status.as_deref(), Some("delivered"));
        assert!(analyzed.recommendations.is_empty());
        assert_eq!(
            store.get_alert(analyzed_id).unwrap().recommendations.len(),
            2
        );
    }

    #[test]
    fn retries_failed_candidate_with_its_persisted_context() {
        let directory = tempdir().unwrap();
        let mut store = AlertStore::open(&directory.path().join("alerts.db")).unwrap();
        let mut context = TriggerContext::for_summary(&[], &[], &[]);
        context.triggered_by = "CrashDetectionRule".to_string();
        context.trigger_source = Some("ExampleEditor".to_string());
        context.trigger_reason = "ExampleEditor crashed".to_string();
        context.expected_severity = Severity::Critical;
        context.log_events.push(LogEvent {
            timestamp: Utc::now(),
            message_type: MessageType::Fault,
            subsystem: "com.example.editor".to_string(),
            category: "crash".to_string(),
            process: "ExampleEditor".to_string(),
            process_id: 42,
            message: "Application terminated unexpectedly".to_string(),
        });
        let candidate_id = store.record_candidate(&context).unwrap();
        store
            .mark_candidate_failed(candidate_id, "backend unavailable")
            .unwrap();

        let retried = store.retry_candidate(candidate_id).unwrap();

        assert_eq!(retried.triggered_by, context.triggered_by);
        assert_eq!(retried.trigger_source, context.trigger_source);
        assert_eq!(retried.expected_severity, Severity::Critical);
        assert_eq!(retried.log_events, context.log_events);
        let page = store
            .list_alerts(1, 10, AlertSort::AssessedAt, true)
            .unwrap();
        assert_eq!(page.alerts[0].analysis_status, "pending");
        assert!(page.alerts[0].analysis_failure.is_none());
        assert!(matches!(
            store.retry_candidate(candidate_id),
            Err(AlertError::CandidateNotRetryable { .. })
        ));
    }

    #[test]
    fn migrates_v1_alerts_to_analyzed_candidates() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("alerts.db");
        let mut store = AlertStore::open(&database_path).unwrap();
        store
            .record_alert(
                &test_insight(),
                "System Alert",
                "Notification body",
                AlertStatus::Delivered,
            )
            .unwrap();
        store
            .connection
            .execute_batch(
                "DROP TABLE alert_candidate_context_events;
                 DROP TABLE alert_candidates;
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(store);

        let migrated = AlertStore::open(&database_path).unwrap();
        let page = migrated
            .list_alerts(1, 10, AlertSort::AssessedAt, true)
            .unwrap();
        assert_eq!(page.alerts.len(), 1);
        assert_eq!(page.alerts[0].analysis_status, "analyzed");
        assert_eq!(page.alerts[0].triggered_by, "legacy_alert");
        assert_eq!(page.alerts[0].summary, "Memory pressure");
    }
}
