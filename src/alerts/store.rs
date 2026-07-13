use crate::ai::AIInsight;
use crate::error::AlertError;
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Duration;

const SCHEMA_VERSION: i32 = 1;

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
        transaction.commit().map_err(persistence_error)?;
        Ok(alert_id)
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

    fn migrate(&mut self) -> Result<(), AlertError> {
        let version = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            .map_err(persistence_error)?;

        if version > SCHEMA_VERSION {
            return Err(AlertError::PersistenceFailed(format!(
                "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            )));
        }

        if version == 0 {
            self.connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     CREATE TABLE assessments (
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
                     PRAGMA user_version = 1;
                     COMMIT;",
                )
                .map_err(persistence_error)?;
        }

        Ok(())
    }
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
    match insight.severity {
        crate::events::Severity::Info => "info",
        crate::events::Severity::Warning => "warning",
        crate::events::Severity::Critical => "critical",
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
    use crate::events::Severity;
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
}
