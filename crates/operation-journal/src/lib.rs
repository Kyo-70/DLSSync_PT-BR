use dlssync_contracts::{JournalFilter, OperationRecord};
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::PathBuf;

const DEFAULT_LIMIT: u32 = 200;
const MAX_LIMIT: u32 = 1_000;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid journal row: {0}")]
    InvalidRow(String),
}

impl JournalError {
    pub fn is_missing_record(&self) -> bool {
        matches!(self, Self::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    }
}

#[derive(Debug, Clone)]
pub struct JournalStore {
    db_path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecoveryMember {
    pub target: PathBuf,
    pub backup: PathBuf,
    pub previous_sha256: String,
    pub expected_sha256: String,
    pub stage: dlssync_contracts::OperationStage,
    #[serde(default)]
    pub game_id: Option<String>,
    #[serde(default)]
    pub component_id: Option<String>,
    #[serde(default)]
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecoveryRecord {
    pub id: String,
    pub plan_id: String,
    pub actor: dlssync_contracts::OperationActor,
    pub stage: dlssync_contracts::OperationStage,
    pub members: Vec<RecoveryMember>,
    pub error: Option<String>,
    #[serde(default)]
    pub source_store_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub game_ids: Vec<String>,
    #[serde(default)]
    pub kind: RecoveryKind,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    #[default]
    Apply,
    ExplicitRestore,
    RollbackOnError,
    StartupRecovery,
    Unknown,
}

impl JournalStore {
    pub fn open(db_path: PathBuf) -> Result<Self, JournalError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { db_path };
        store.ensure_schema()?;
        Ok(store)
    }

    fn connection(&self) -> Result<rusqlite::Connection, JournalError> {
        let connection = rusqlite::Connection::open(&self.db_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA synchronous = FULL;")?;
        Ok(connection)
    }

    fn ensure_schema(&self) -> Result<(), JournalError> {
        self.connection()?.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS file_transactions (
                 id TEXT PRIMARY KEY,
                 stage TEXT NOT NULL,
                 payload TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS recovery_supersessions (
                 id TEXT NOT NULL, target TEXT NOT NULL, decision TEXT NOT NULL,
                 original_payload TEXT NOT NULL,
                 PRIMARY KEY(id, target)
             );
             CREATE TABLE IF NOT EXISTS pending_targets (
                 journal TEXT NOT NULL, id TEXT NOT NULL, target TEXT NOT NULL,
                 decision TEXT,
                 PRIMARY KEY(journal, id, target)
             );
             CREATE INDEX IF NOT EXISTS idx_pending_targets_target ON pending_targets(target);
              CREATE TABLE IF NOT EXISTS operations (
                 id TEXT PRIMARY KEY,
                 created_at TEXT NOT NULL,
                 actor TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 status TEXT NOT NULL,
                 target TEXT,
                 summary TEXT NOT NULL,
                 details_json TEXT NOT NULL,
                 duration_ms INTEGER,
                 backup_id TEXT,
                 error TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_operations_created
                 ON operations(created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_operations_target
                 ON operations(target, created_at DESC);
              CREATE INDEX IF NOT EXISTS idx_operations_kind_status
                  ON operations(kind, status, created_at DESC);
              CREATE TABLE IF NOT EXISTS projection_ledger (
                  source_store_id TEXT NOT NULL,
                  source_record_id TEXT NOT NULL,
                  projection_id TEXT NOT NULL,
                  projected_at TEXT NOT NULL,
                  tombstoned INTEGER NOT NULL DEFAULT 0,
                  PRIMARY KEY(source_store_id, source_record_id)
              );",
        )?;
        Ok(())
    }

    /// The complete recovery record commits before each write boundary.
    /// This table is deliberately independent of history pruning.
    pub fn save_recovery(&self, record: &RecoveryRecord) -> Result<(), JournalError> {
        self.connection()?.execute(
            "INSERT INTO file_transactions(id, stage, payload) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET stage=excluded.stage, payload=excluded.payload",
            params![
                record.id,
                serde_json::to_string(&record.stage)?,
                serde_json::to_string(record)?
            ],
        )?;
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path {
        &self.db_path
    }

    /// An additive audit trail: original recovery payloads and errors stay intact.
    pub fn supersede_member(
        &self,
        id: &str,
        target: &str,
        decision: &str,
    ) -> Result<(), JournalError> {
        self.connection()?.execute(
            "INSERT OR IGNORE INTO recovery_supersessions(id,target,decision,original_payload)
             SELECT ?1,?2,?3,payload FROM file_transactions WHERE id=?1",
            params![id, target, decision],
        )?;
        Ok(())
    }

    pub fn member_superseded(&self, id: &str, target: &str) -> Result<bool, JournalError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM recovery_supersessions WHERE id=?1 AND target=?2)",
            params![id, target],
            |row| row.get(0),
        )?)
    }

    /// The shared index is committed before the owning journal/write boundary.
    pub fn register_target(
        &self,
        journal: &str,
        id: &str,
        target: &str,
    ) -> Result<(), JournalError> {
        self.connection()?.execute(
            "INSERT OR IGNORE INTO pending_targets(journal,id,target) VALUES (?1,?2,?3)",
            params![journal, id, target],
        )?;
        Ok(())
    }

    pub fn indexed_targets(
        &self,
        target: &str,
    ) -> Result<Vec<(String, String, Option<String>)>, JournalError> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare("SELECT journal,id,decision FROM pending_targets WHERE target=?1")?;
        let rows = query.query_map([target], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn supersede_target(
        &self,
        journal: &str,
        id: &str,
        target: &str,
        decision: &str,
    ) -> Result<(), JournalError> {
        self.connection()?.execute(
            "UPDATE pending_targets SET decision=?4 WHERE journal=?1 AND id=?2 AND target=?3 AND decision IS NULL",
            params![journal, id, target, decision],
        )?;
        Ok(())
    }

    pub fn recovery_record(&self, id: &str) -> Result<RecoveryRecord, JournalError> {
        let payload: String = self.connection()?.query_row(
            "SELECT payload FROM file_transactions WHERE id=?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }

    pub fn pending_recovery(&self) -> Result<Vec<RecoveryRecord>, JournalError> {
        let connection = self.connection()?;
        let mut query = connection.prepare("SELECT payload FROM file_transactions ORDER BY id")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?;
        let mut pending = Vec::new();
        for row in rows {
            let record: RecoveryRecord = serde_json::from_str(&row?)?;
            if !record.stage.is_terminal()
                || record.stage == dlssync_contracts::OperationStage::RollbackFailed
            {
                let mut unresolved = false;
                for member in &record.members {
                    if !self.member_superseded(&record.id, &member.target.to_string_lossy())? {
                        unresolved = true;
                        break;
                    }
                }
                if unresolved {
                    pending.push(record);
                }
            }
        }
        Ok(pending)
    }

    pub fn all_recovery_records(&self) -> Result<Vec<RecoveryRecord>, JournalError> {
        let connection = self.connection()?;
        let mut query = connection.prepare("SELECT payload FROM file_transactions ORDER BY id")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn source_store_id(&self) -> Result<String, JournalError> {
        let connection = self.connection()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS store_identity (
                 singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                 id TEXT NOT NULL
             );",
        )?;
        if let Ok(id) = connection.query_row(
            "SELECT id FROM store_identity WHERE singleton=1",
            [],
            |row| row.get::<_, String>(0),
        ) {
            return Ok(id);
        }
        let id = format!("store-{}", new_identity_suffix());
        connection.execute(
            "INSERT OR IGNORE INTO store_identity(singleton,id) VALUES (1,?1)",
            [&id],
        )?;
        Ok(connection.query_row(
            "SELECT id FROM store_identity WHERE singleton=1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn project_recovery_once(
        &self,
        source_store_id: &str,
        recovery: &RecoveryRecord,
        record: &OperationRecord,
    ) -> Result<bool, JournalError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM projection_ledger
                 WHERE source_store_id=?1 AND source_record_id=?2
             )",
            params![source_store_id, recovery.id],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO operations
             (id,created_at,actor,kind,status,target,summary,details_json,duration_ms,backup_id,error)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET
               created_at=excluded.created_at,actor=excluded.actor,kind=excluded.kind,
               status=excluded.status,target=excluded.target,summary=excluded.summary,
               details_json=excluded.details_json,duration_ms=excluded.duration_ms,
               backup_id=excluded.backup_id,error=excluded.error",
            params![
                record.id,
                record.created_at,
                record.actor.as_str(),
                record.kind.as_str(),
                record.status.as_str(),
                record.target,
                record.summary,
                serde_json::to_string(&record.details)?,
                record.duration_ms,
                record.backup_id,
                record.error,
            ],
        )?;
        transaction.execute(
            "INSERT INTO projection_ledger
             (source_store_id,source_record_id,projection_id,projected_at,tombstoned)
             VALUES (?1,?2,?3,?4,0)",
            params![source_store_id, recovery.id, record.id, record.created_at],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn append(&self, record: &OperationRecord) -> Result<(), JournalError> {
        self.connection()?.execute(
            "INSERT INTO operations
                (id, created_at, actor, kind, status, target, summary, details_json,
                 duration_ms, backup_id, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.id,
                record.created_at,
                record.actor.as_str(),
                record.kind.as_str(),
                record.status.as_str(),
                record.target,
                record.summary,
                serde_json::to_string(&record.details)?,
                record.duration_ms,
                record.backup_id,
                record.error,
            ],
        )?;
        Ok(())
    }

    pub fn list(&self, filter: &JournalFilter) -> Result<Vec<OperationRecord>, JournalError> {
        let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, actor, kind, status, target, summary, details_json,
                    duration_ms, backup_id, error
             FROM operations ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([MAX_LIMIT], row_to_record)?;
        let mut records = Vec::new();
        for row in rows {
            let record = row?;
            if filter
                .target
                .as_ref()
                .is_some_and(|target| record.target.as_deref() != Some(target.as_str()))
            {
                continue;
            }
            if filter.kind.is_some_and(|kind| record.kind != kind) {
                continue;
            }
            if filter.status.is_some_and(|status| record.status != status) {
                continue;
            }
            records.push(record);
            if records.len() == limit {
                break;
            }
        }
        Ok(records)
    }

    pub fn list_all(&self) -> Result<Vec<OperationRecord>, JournalError> {
        let connection = self.connection()?;
        let mut query = connection.prepare(
            "SELECT id, created_at, actor, kind, status, target, summary, details_json,
                    duration_ms, backup_id, error
             FROM operations ORDER BY created_at DESC, id DESC",
        )?;
        let rows = query.query_map([], row_to_record)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn export_redacted_json(&self, filter: &JournalFilter) -> Result<String, JournalError> {
        let mut records = self.list(filter)?;
        for record in &mut records {
            record.target = record
                .target
                .as_ref()
                .map(|_| "[redacted-target]".to_string());
            for (key, value) in &mut record.details {
                if is_sensitive_key(key) || looks_like_local_path(value) {
                    *value = "[redacted]".to_string();
                }
            }
            if looks_like_local_path(&record.summary) {
                record.summary = "[redacted-summary-with-local-path]".to_string();
            }
            if record
                .error
                .as_ref()
                .is_some_and(|value| looks_like_local_path(value))
            {
                record.error = Some("[redacted-error-with-local-path]".into());
            }
        }
        Ok(serde_json::to_string_pretty(&records)?)
    }

    pub fn prune_unlinked(&self, keep: u32) -> Result<usize, JournalError> {
        let changed = self.connection()?.execute(
            "DELETE FROM operations
             WHERE backup_id IS NULL
               AND id IN (
                 SELECT id FROM operations
                 WHERE backup_id IS NULL
                 ORDER BY created_at DESC
                 LIMIT -1 OFFSET ?1
               )",
            [keep],
        )?;
        Ok(changed)
    }
}

fn new_identity_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{now:032x}-{sequence:016x}")
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRecord> {
    let actor: String = row.get(2)?;
    let kind: String = row.get(3)?;
    let status: String = row.get(4)?;
    let details_json: String = row.get(7)?;
    Ok(OperationRecord {
        id: row.get(0)?,
        created_at: row.get(1)?,
        actor: parse_enum(&actor).map_err(to_sql_error)?,
        kind: parse_enum(&kind).map_err(to_sql_error)?,
        status: parse_enum(&status).map_err(to_sql_error)?,
        target: row.get(5)?,
        summary: row.get(6)?,
        details: serde_json::from_str::<BTreeMap<String, String>>(&details_json)
            .map_err(to_sql_error)?,
        duration_ms: row.get(8)?,
        backup_id: row.get(9)?,
        error: row.get(10)?,
    })
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(&serde_json::to_string(value)?)
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("path") || key.contains("token") || key.contains("secret") || key.contains("key")
}

/// Value-based detector for local filesystem paths. Conservative by design: it aims
/// to match real local paths (Windows drive/UNC/env-var, home-tilde, and POSIX
/// absolute user paths) while leaving ordinary prose and web URLs intact.
fn looks_like_local_path(value: &str) -> bool {
    // Never treat web URLs as local paths.
    let trimmed = value.trim_start();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return false;
    }

    let bytes = value.as_bytes();

    // UNC paths, e.g. \\server\share
    if value.contains("\\\\") {
        return true;
    }

    // Windows drive paths anywhere in the value, e.g. C:\ or D:/ — but never a URL
    // scheme like `x://`. The letter must stand alone as a drive letter.
    for i in 1..bytes.len() {
        if bytes[i] != b':' {
            continue;
        }
        if !bytes[i - 1].is_ascii_alphabetic() {
            continue;
        }
        let boundary = i == 1 || !bytes[i - 2].is_ascii_alphanumeric();
        if !boundary {
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'\\') => return true,
            // `C:/path` is a drive path, but `://` (a URL scheme) is not.
            Some(b'/') if bytes.get(i + 2) != Some(&b'/') => return true,
            _ => {}
        }
    }

    // Windows environment-variable directories.
    if value.contains("%APPDATA%")
        || value.contains("%LOCALAPPDATA%")
        || value.contains("%USERPROFILE%")
        || contains_env_var_dir(value)
    {
        return true;
    }

    // Home-relative path, e.g. ~/Library/...
    if value.contains("~/") {
        return true;
    }

    // Well-known POSIX user/system directories.
    if value.contains("/home/")
        || value.contains("/Users/")
        || value.contains("/mnt/")
        || value.contains("/root/")
    {
        return true;
    }

    // A leading absolute POSIX path with at least two segments, e.g. /etc/hosts.
    if let Some(rest) = value.strip_prefix('/') {
        if let Some(idx) = rest.find('/') {
            if idx > 0 {
                return true;
            }
        }
    }

    false
}

/// Matches a generic Windows env-var expansion followed by a path separator, e.g. `%VAR%\`.
fn contains_env_var_dir(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut open: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'%' {
            continue;
        }
        match open {
            // A closing `%` with a non-empty name in between, followed by `\`.
            Some(start) if i > start + 1 => {
                if bytes.get(i + 1) == Some(&b'\\') {
                    return true;
                }
                open = Some(i);
            }
            _ => open = Some(i),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use dlssync_contracts::{OperationActor, OperationKind, OperationStatus};
    use tempfile::tempdir;

    fn record(id: &str, backup_id: Option<&str>) -> OperationRecord {
        OperationRecord {
            id: id.into(),
            created_at: format!("2026-07-10T00:00:0{id}Z"),
            actor: OperationActor::Gui,
            kind: OperationKind::DllApply,
            status: OperationStatus::Succeeded,
            target: Some("C:\\Games\\Private\\game.dll".into()),
            summary: "Updated DLSS".into(),
            details: BTreeMap::from([
                ("source_url".into(), "https://vendor.example/file".into()),
                ("backup_path".into(), "C:\\Users\\Tony\\Backup".into()),
            ]),
            duration_ms: Some(42),
            backup_id: backup_id.map(str::to_string),
            error: None,
        }
    }

    #[test]
    fn legacy_recovery_schema_upgrades_and_supersession_keeps_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use dlssync_contracts::OperationStage;
        let dir = tempdir()?;
        let path = dir.path().join("legacy.db");
        let record = RecoveryRecord {
            id: "failed".into(),
            plan_id: "plan".into(),
            actor: OperationActor::Gui,
            stage: OperationStage::RollbackFailed,
            error: Some("original rollback failure".into()),
            source_store_id: None,
            operation_id: None,
            game_ids: Vec::new(),
            kind: RecoveryKind::Unknown,
            started_at: None,
            updated_at: None,
            members: vec![RecoveryMember {
                target: dir.path().join("game.dll"),
                backup: dir.path().join("backup.dll"),
                previous_sha256: "previous".into(),
                expected_sha256: "expected".into(),
                stage: OperationStage::RollbackFailed,
                game_id: None,
                component_id: None,
                backup_id: None,
            }],
        };
        let payload = serde_json::to_string(&record)?;
        {
            let connection = rusqlite::Connection::open(&path)?;
            connection.execute_batch("CREATE TABLE file_transactions(id TEXT PRIMARY KEY, stage TEXT NOT NULL, payload TEXT NOT NULL);")?;
            connection.execute(
                "INSERT INTO file_transactions VALUES (?1,?2,?3)",
                params![record.id, serde_json::to_string(&record.stage)?, payload],
            )?;
        }
        let journal = JournalStore::open(path)?;
        assert_eq!(journal.pending_recovery()?.len(), 1);
        let target = record.members[0].target.to_string_lossy();
        journal.supersede_member(&record.id, &target, "verified restore: new-operation")?;
        assert!(journal.pending_recovery()?.is_empty());
        assert_eq!(journal.recovery_record(&record.id)?.error, record.error);
        // A later recovery of a different package member cannot erase the snapshot.
        let mut later = record.clone();
        later.error = Some("later diagnostic".into());
        journal.save_recovery(&later)?;
        journal.supersede_member(&record.id, &target, "second decision")?;
        let (decision, original): (String, String) = journal.connection()?.query_row(
            "SELECT decision,original_payload FROM recovery_supersessions WHERE id=?1 AND target=?2",
            params![record.id, target], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(decision, "verified restore: new-operation");
        assert_eq!(original, payload);
        let synchronous: i64 =
            journal
                .connection()?
                .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        assert_eq!(synchronous, 2);
        Ok(())
    }

    #[test]
    fn append_list_filter_and_redacted_export_round_trip() {
        let dir = tempdir().unwrap();
        let store = JournalStore::open(dir.path().join("journal.db")).unwrap();
        store.append(&record("1", Some("backup-1"))).unwrap();
        let rows = store.list(&JournalFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].backup_id.as_deref(), Some("backup-1"));
        let export = store
            .export_redacted_json(&JournalFilter::default())
            .unwrap();
        assert!(export.contains("[redacted-target]"));
        assert!(export.contains("[redacted]"));
        assert!(!export.contains("Tony"));
    }

    #[test]
    fn authoritative_list_all_is_not_capped_by_the_ui_limit() {
        let dir = tempdir().unwrap();
        let store = JournalStore::open(dir.path().join("journal.db")).unwrap();
        for index in 0..=DEFAULT_LIMIT {
            let mut item = record(&format!("all-{index:03}"), None);
            item.created_at = format!("2026-09-18T00:{:02}:{:02}Z", (index / 60) % 60, index % 60);
            store.append(&item).unwrap();
        }
        assert_eq!(store.list(&JournalFilter::default()).unwrap().len(), 200);
        assert_eq!(store.list_all().unwrap().len(), 201);
    }

    fn record_with(
        id: &str,
        summary: &str,
        error: Option<&str>,
        details: BTreeMap<String, String>,
    ) -> OperationRecord {
        OperationRecord {
            id: id.into(),
            created_at: format!("2026-07-10T00:00:0{id}Z"),
            actor: OperationActor::Gui,
            kind: OperationKind::DllApply,
            status: OperationStatus::Succeeded,
            target: None,
            summary: summary.into(),
            details,
            duration_ms: Some(1),
            backup_id: None,
            error: error.map(str::to_string),
        }
    }

    fn export_one(record: OperationRecord) -> String {
        let dir = tempdir().unwrap();
        let store = JournalStore::open(dir.path().join("journal.db")).unwrap();
        store.append(&record).unwrap();
        store
            .export_redacted_json(&JournalFilter::default())
            .unwrap()
    }

    #[test]
    fn redacts_windows_path_detail_under_non_sensitive_key() {
        let details =
            BTreeMap::from([("note".into(), "C:\\Users\\bob\\game\\nvngx_dlss.dll".into())]);
        let export = export_one(record_with("1", "ok", None, details));
        assert!(export.contains("[redacted]"));
        assert!(!export.contains("nvngx_dlss.dll"));
        assert!(!export.contains("bob"));
        // The key name itself is preserved — only the value is redacted.
        assert!(export.contains("\"note\""));
    }

    #[test]
    fn redacts_posix_path_detail_under_non_sensitive_key() {
        let details = BTreeMap::from([("game".into(), "/home/bob/games/x/libxess.dll".into())]);
        let export = export_one(record_with("1", "ok", None, details));
        assert!(export.contains("[redacted]"));
        assert!(!export.contains("libxess.dll"));
        assert!(!export.contains("/home/bob"));
    }

    #[test]
    fn redacts_error_containing_posix_path() {
        let export = export_one(record_with(
            "1",
            "ok",
            Some("failed to write /Users/bob/Game/dll"),
            BTreeMap::new(),
        ));
        assert!(export.contains("[redacted-error-with-local-path]"));
        assert!(!export.contains("/Users/bob"));
    }

    #[test]
    fn redacts_summary_containing_local_path() {
        let export = export_one(record_with(
            "1",
            "Copied to C:\\Users\\bob\\game",
            None,
            BTreeMap::new(),
        ));
        assert!(export.contains("[redacted-summary-with-local-path]"));
        assert!(!export.contains("Copied to C"));
        assert!(!export.contains("bob"));
    }

    #[test]
    fn preserves_normal_summary_and_detail_values() {
        let details = BTreeMap::from([("count".into(), "4".into())]);
        let export = export_one(record_with("1", "Library scan completed", None, details));
        assert!(export.contains("Library scan completed"));
        assert!(!export.contains("[redacted-summary-with-local-path]"));
        assert!(export.contains("\"4\""));
        assert!(!export.contains("[redacted]"));
    }

    #[test]
    fn still_redacts_sensitive_key_values() {
        let details = BTreeMap::from([("api_key".into(), "secret-value-123".into())]);
        let export = export_one(record_with("1", "ok", None, details));
        assert!(export.contains("[redacted]"));
        assert!(!export.contains("secret-value-123"));
    }

    #[test]
    fn prune_keeps_backup_linked_operations() {
        let dir = tempdir().unwrap();
        let store = JournalStore::open(dir.path().join("journal.db")).unwrap();
        store.append(&record("1", None)).unwrap();
        store.append(&record("2", None)).unwrap();
        store.append(&record("3", Some("backup-3"))).unwrap();
        assert_eq!(store.prune_unlinked(1).unwrap(), 1);
        let rows = store
            .list(&JournalFilter {
                limit: Some(10),
                ..JournalFilter::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|row| row.backup_id.as_deref() == Some("backup-3")));
    }

    #[test]
    fn phase4_projection_is_idempotent_and_survives_history_pruning() {
        let dir = tempdir().unwrap();
        let recovery_store = JournalStore::open(dir.path().join("recovery.db")).unwrap();
        let history_store = JournalStore::open(dir.path().join("history.db")).unwrap();
        let source_store_id = recovery_store.source_store_id().unwrap();
        let recovery = RecoveryRecord {
            id: "recovery-1".into(),
            plan_id: "plan-1".into(),
            actor: OperationActor::Gui,
            stage: dlssync_contracts::OperationStage::Completed,
            members: Vec::new(),
            error: None,
            source_store_id: Some(source_store_id.clone()),
            operation_id: Some("operation-1".into()),
            game_ids: vec!["game-1".into()],
            kind: RecoveryKind::Apply,
            started_at: Some("2026-09-17T00:00:00Z".into()),
            updated_at: Some("2026-09-17T00:00:01Z".into()),
        };
        recovery_store.save_recovery(&recovery).unwrap();
        let projected = OperationRecord {
            id: "projection:recovery-1".into(),
            created_at: "2026-09-17T00:00:00Z".into(),
            actor: OperationActor::Gui,
            kind: OperationKind::DllApply,
            status: OperationStatus::Succeeded,
            target: Some("game-1".into()),
            summary: "Recovered durable apply".into(),
            details: BTreeMap::new(),
            duration_ms: None,
            backup_id: None,
            error: None,
        };

        assert!(history_store
            .project_recovery_once(&source_store_id, &recovery, &projected)
            .unwrap());
        assert!(!history_store
            .project_recovery_once(&source_store_id, &recovery, &projected)
            .unwrap());
        assert_eq!(history_store.prune_unlinked(0).unwrap(), 1);
        assert!(!history_store
            .project_recovery_once(&source_store_id, &recovery, &projected)
            .unwrap());
        assert_eq!(recovery_store.all_recovery_records().unwrap().len(), 1);
    }

    #[test]
    fn phase4_legacy_recovery_payload_defaults_new_metadata() {
        let payload = r#"{
            "id":"legacy","plan_id":"plan","actor":"gui","stage":"completed",
            "members":[],"error":null
        }"#;
        let recovery: RecoveryRecord = serde_json::from_str(payload).unwrap();
        assert_eq!(recovery.kind, RecoveryKind::Apply);
        assert!(recovery.source_store_id.is_none());
        assert!(recovery.operation_id.is_none());
        assert!(recovery.game_ids.is_empty());
        assert!(recovery.started_at.is_none());
    }
}
