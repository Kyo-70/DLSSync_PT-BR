use crate::ports::{StateClock, StateEventPublisher};
use dlssync_contracts::{
    AuthoritativeSnapshot, BackupAvailability, BackupKind, BackupView, Counter, GameStateStatus,
    HistoryView, OperationKind, OperationStage, OperationStatus, RestoreEligibilityView,
    RestoreVerification, StateCounts, StateDelta, StateEvent,
};
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

pub const STATE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateError {
    #[error("stale observation for {scope}: expected generation {expected}, received {received}")]
    StaleObservation {
        scope: String,
        expected: u64,
        received: u64,
    },
    #[error("observation emitter does not match the current coordinator epoch")]
    ForeignEmitter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationTicket {
    emitter_id: String,
    scope: String,
    generation: u64,
    scope_revision: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StateCommit {
    pub operation_id: Option<String>,
    pub game_id: Option<String>,
    pub delta: StateDelta,
}

impl StateCommit {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn for_game(game_id: impl Into<String>) -> Self {
        let game_id = game_id.into();
        Self {
            game_id: Some(game_id.clone()),
            delta: StateDelta {
                affected_game_ids: vec![game_id],
                ..StateDelta::default()
            },
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitReceipt {
    pub event: StateEvent,
    pub delivery_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDisposition {
    Apply,
    Duplicate,
    ForeignEmitter,
    Resnapshot,
}

pub fn validate_next_event(
    snapshot: &AuthoritativeSnapshot,
    event: &StateEvent,
) -> EventDisposition {
    if event.emitter_id != snapshot.emitter_id {
        return EventDisposition::ForeignEmitter;
    }
    let Some(snapshot_sequence) = snapshot.sequence.parse() else {
        return EventDisposition::Resnapshot;
    };
    let Some(event_sequence) = event.sequence.parse() else {
        return EventDisposition::Resnapshot;
    };
    if event_sequence <= snapshot_sequence {
        return EventDisposition::Duplicate;
    }
    if event_sequence != snapshot_sequence.saturating_add(1)
        || event.delta.base_revision != snapshot.revision
        || event.revision.parse() != snapshot.revision.parse().and_then(|v| v.checked_add(1))
    {
        return EventDisposition::Resnapshot;
    }
    EventDisposition::Apply
}

struct CoordinatorState {
    snapshot: AuthoritativeSnapshot,
    operation_sequences: HashMap<String, u64>,
    observation_generations: HashMap<String, u64>,
    scope_revisions: HashMap<String, u64>,
}

pub struct StateCoordinator {
    emitter_id: String,
    initial_snapshot: AuthoritativeSnapshot,
    state: RwLock<CoordinatorState>,
    commit_gate: Mutex<()>,
    publisher: Arc<dyn StateEventPublisher>,
    clock: Arc<dyn StateClock>,
}

impl StateCoordinator {
    pub fn new(publisher: Arc<dyn StateEventPublisher>, clock: Arc<dyn StateClock>) -> Self {
        Self::with_emitter(uuid::Uuid::new_v4().to_string(), publisher, clock)
    }

    pub fn with_emitter(
        emitter_id: String,
        publisher: Arc<dyn StateEventPublisher>,
        clock: Arc<dyn StateClock>,
    ) -> Self {
        let snapshot = AuthoritativeSnapshot {
            schema_version: STATE_SCHEMA_VERSION,
            emitter_id: emitter_id.clone(),
            sequence: Counter::from(0),
            revision: Counter::from(0),
            captured_at: clock.now_utc(),
            ..AuthoritativeSnapshot::default()
        };
        Self {
            emitter_id,
            initial_snapshot: snapshot.clone(),
            state: RwLock::new(CoordinatorState {
                snapshot,
                operation_sequences: HashMap::new(),
                observation_generations: HashMap::new(),
                scope_revisions: HashMap::new(),
            }),
            commit_gate: Mutex::new(()),
            publisher,
            clock,
        }
    }

    pub fn begin_observation(&self, scope: impl Into<String>) -> ObservationTicket {
        let scope = scope.into();
        let mut state = self.state.write();
        let generation = state
            .observation_generations
            .get(&scope)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        state
            .observation_generations
            .insert(scope.clone(), generation);
        ObservationTicket {
            emitter_id: self.emitter_id.clone(),
            scope_revision: state.scope_revisions.get(&scope).copied().unwrap_or(0),
            scope,
            generation,
        }
    }

    pub fn commit_observation(
        &self,
        ticket: ObservationTicket,
        commit: StateCommit,
    ) -> Result<CommitReceipt, StateError> {
        let _gate = self.commit_gate.lock();
        if ticket.emitter_id != self.emitter_id {
            return Err(StateError::ForeignEmitter);
        }
        {
            let state = self.state.read();
            let expected = state
                .observation_generations
                .get(&ticket.scope)
                .copied()
                .unwrap_or(0);
            let scope_revision = state
                .scope_revisions
                .get(&ticket.scope)
                .copied()
                .unwrap_or(0);
            if expected != ticket.generation || scope_revision != ticket.scope_revision {
                return Err(StateError::StaleObservation {
                    scope: ticket.scope,
                    expected,
                    received: ticket.generation,
                });
            }
        }
        self.commit_locked(commit, Some(ticket.scope))
    }

    pub fn commit(&self, commit: StateCommit) -> Result<CommitReceipt, StateError> {
        let _gate = self.commit_gate.lock();
        self.commit_locked(commit, None)
    }

    pub fn refresh_persisted_views(
        &self,
        backups: Option<&backup_store::BackupStore>,
        history: Option<&operation_journal::JournalStore>,
    ) -> Result<Option<CommitReceipt>, String> {
        let _gate = self.commit_gate.lock();
        let snapshot = self.snapshot();
        let (mut projected_backups, mut projected_history) =
            project_persisted_views(backups, history, &snapshot.operations)?;
        if backups.is_none() {
            projected_backups = snapshot.backups.clone();
        }
        if history.is_none() {
            projected_history = snapshot.history.clone();
        }
        if same_persisted_views(
            &snapshot.backups,
            &projected_backups,
            &snapshot.history,
            &projected_history,
        ) {
            return Ok(None);
        }
        let projected_backup_ids: BTreeSet<_> =
            projected_backups.iter().map(|row| row.id.clone()).collect();
        let projected_history_ids: BTreeSet<_> =
            projected_history.iter().map(|row| row.id.clone()).collect();
        let receipt = self
            .commit_locked(
                StateCommit {
                    delta: StateDelta {
                        backups: projected_backups,
                        removed_backup_ids: snapshot
                            .backups
                            .iter()
                            .filter(|row| !projected_backup_ids.contains(&row.id))
                            .map(|row| row.id.clone())
                            .collect(),
                        history: projected_history,
                        removed_history_ids: snapshot
                            .history
                            .iter()
                            .filter(|row| !projected_history_ids.contains(&row.id))
                            .map(|row| row.id.clone())
                            .collect(),
                        ..StateDelta::default()
                    },
                    ..StateCommit::default()
                },
                None,
            )
            .map_err(|error| error.to_string())?;
        Ok(Some(receipt))
    }

    fn commit_locked(
        &self,
        mut commit: StateCommit,
        observation_scope: Option<String>,
    ) -> Result<CommitReceipt, StateError> {
        let event = {
            let mut state = self.state.write();
            let base_revision = state.snapshot.revision.parse().unwrap_or(0);
            let next_revision = base_revision.saturating_add(1);
            let next_sequence = state
                .snapshot
                .sequence
                .parse()
                .unwrap_or(0)
                .saturating_add(1);
            commit.delta.base_revision = Counter::from(base_revision);

            for game in &mut commit.delta.games {
                game.revision = Counter::from(next_revision);
                for component in &mut game.components {
                    component.revision = next_revision.to_string();
                }
            }
            for operation in &mut commit.delta.operations {
                let sequence = state
                    .operation_sequences
                    .get(&operation.id)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1);
                state
                    .operation_sequences
                    .insert(operation.id.clone(), sequence);
                operation.sequence = Counter::from(sequence);
                operation.state_revision = next_revision.to_string();
                commit
                    .delta
                    .affected_game_ids
                    .extend(operation.game_ids.iter().cloned());
            }
            for backup in &mut commit.delta.backups {
                backup.revision = Counter::from(next_revision);
            }
            for history in &mut commit.delta.history {
                history.revision = Counter::from(next_revision);
            }
            commit.delta.affected_game_ids.sort();
            commit.delta.affected_game_ids.dedup();

            upsert_by(
                &mut state.snapshot.games,
                commit.delta.games.clone(),
                |value| value.id.as_str(),
            );
            remove_by(
                &mut state.snapshot.games,
                &commit.delta.removed_game_ids,
                |value| value.id.as_str(),
            );
            upsert_by(
                &mut state.snapshot.operations,
                commit.delta.operations.clone(),
                |value| value.id.as_str(),
            );
            remove_by(
                &mut state.snapshot.operations,
                &commit.delta.removed_operation_ids,
                |value| value.id.as_str(),
            );
            upsert_by(
                &mut state.snapshot.backups,
                commit.delta.backups.clone(),
                |value| value.id.as_str(),
            );
            remove_by(
                &mut state.snapshot.backups,
                &commit.delta.removed_backup_ids,
                |value| value.id.as_str(),
            );
            upsert_by(
                &mut state.snapshot.history,
                commit.delta.history.clone(),
                |value| value.id.as_str(),
            );
            remove_by(
                &mut state.snapshot.history,
                &commit.delta.removed_history_ids,
                |value| value.id.as_str(),
            );
            if let Some(catalog) = &commit.delta.catalog {
                state.snapshot.catalog = catalog.clone();
            }
            let operations = state.snapshot.operations.clone();
            for backup in &mut state.snapshot.backups {
                let eligibility = restore_eligibility(
                    backup.backup_path.is_empty(),
                    backup.availability,
                    backup_operation_in_flight(backup, &operations),
                );
                if backup.restore_eligibility != eligibility {
                    backup.restore_eligibility = eligibility;
                    backup.revision = Counter::from(next_revision);
                    if let Some(delta_backup) = commit
                        .delta
                        .backups
                        .iter_mut()
                        .find(|candidate| candidate.id == backup.id)
                    {
                        *delta_backup = backup.clone();
                    } else {
                        commit.delta.backups.push(backup.clone());
                    }
                }
            }
            commit
                .delta
                .backups
                .sort_by(|left, right| left.id.cmp(&right.id));
            state.snapshot.counts = reduce_counts(&state.snapshot);
            commit.delta.counts = state.snapshot.counts.clone();
            state.snapshot.sequence = Counter::from(next_sequence);
            state.snapshot.revision = Counter::from(next_revision);
            state.snapshot.captured_at = self.clock.now_utc();

            for game_id in &commit.delta.affected_game_ids {
                state
                    .scope_revisions
                    .insert(format!("game:{game_id}"), next_revision);
            }
            if !commit.delta.games.is_empty() || !commit.delta.removed_game_ids.is_empty() {
                state
                    .scope_revisions
                    .insert("games_projection".to_string(), next_revision);
            }
            if commit.delta.catalog.is_some() {
                state
                    .scope_revisions
                    .insert("catalog".to_string(), next_revision);
            }
            if let Some(scope) = observation_scope {
                state.scope_revisions.insert(scope, next_revision);
            }

            StateEvent {
                schema_version: STATE_SCHEMA_VERSION,
                id: uuid::Uuid::new_v4().to_string(),
                emitter_id: self.emitter_id.clone(),
                sequence: Counter::from(next_sequence),
                operation_id: commit.operation_id,
                game_id: commit.game_id,
                revision: Counter::from(next_revision),
                emitted_at: self.clock.now_utc(),
                delta: commit.delta,
            }
        };

        let delivery_error = self.publisher.publish(&event).err();
        Ok(CommitReceipt {
            event,
            delivery_error,
        })
    }

    pub fn snapshot(&self) -> AuthoritativeSnapshot {
        let mut snapshot = self.state.read().snapshot.clone();
        snapshot.captured_at = self.clock.now_utc();
        snapshot
    }

    pub fn initial_snapshot(&self) -> AuthoritativeSnapshot {
        self.initial_snapshot.clone()
    }

    pub fn watermark(&self) -> (String, Counter, Counter) {
        let state = self.state.read();
        (
            state.snapshot.emitter_id.clone(),
            state.snapshot.sequence.clone(),
            state.snapshot.revision.clone(),
        )
    }

    #[cfg(test)]
    fn test(emitter_id: &str) -> Self {
        Self::with_emitter(
            emitter_id.to_string(),
            Arc::new(TestPublisher { fail: false }),
            Arc::new(TestClock),
        )
    }

    #[cfg(test)]
    fn test_failing_publisher(emitter_id: &str) -> Self {
        Self::with_emitter(
            emitter_id.to_string(),
            Arc::new(TestPublisher { fail: true }),
            Arc::new(TestClock),
        )
    }
}

fn upsert_by<T: Clone>(target: &mut Vec<T>, values: Vec<T>, key: impl Fn(&T) -> &str) {
    let mut replacements: BTreeMap<String, T> = values
        .into_iter()
        .map(|value| (key(&value).to_string(), value))
        .collect();
    for existing in target.iter_mut() {
        if let Some(replacement) = replacements.remove(key(existing)) {
            *existing = replacement;
        }
    }
    target.extend(replacements.into_values());
    target.sort_by(|left, right| key(left).cmp(key(right)));
}

fn remove_by<T>(target: &mut Vec<T>, removed: &[String], key: impl Fn(&T) -> &str) {
    target.retain(|value| !removed.iter().any(|id| id == key(value)));
}

fn reduce_counts(snapshot: &AuthoritativeSnapshot) -> StateCounts {
    StateCounts {
        actionable_games: snapshot
            .games
            .iter()
            .filter(|game| game.status == GameStateStatus::UpdateAvailable)
            .count() as u32,
        actionable_components: snapshot
            .games
            .iter()
            .flat_map(|game| &game.components)
            .filter(|component| {
                component.status == dlssync_contracts::ComponentStatus::UpdateAvailable
                    && component.applicability == dlssync_contracts::ApplicabilityStatus::Applicable
            })
            .count() as u32,
        protected_games: snapshot
            .backups
            .iter()
            .filter(|backup| {
                backup.kind == BackupKind::GameDll
                    && backup.availability == BackupAvailability::VerifiedPresent
            })
            .filter_map(|backup| backup.game_id.as_deref())
            .collect::<BTreeSet<_>>()
            .len() as u32,
        eligible_restorable_backups: snapshot
            .backups
            .iter()
            .filter(|backup| backup.restore_eligibility.eligible)
            .count() as u32,
        active_operations: snapshot
            .operations
            .iter()
            .filter(|operation| !operation.stage.is_terminal())
            .count() as u32,
    }
}

fn same_persisted_views(
    current_backups: &[BackupView],
    projected_backups: &[BackupView],
    current_history: &[HistoryView],
    projected_history: &[HistoryView],
) -> bool {
    fn normalize_backups(values: &[BackupView]) -> Vec<BackupView> {
        let mut values = values.to_vec();
        for value in &mut values {
            value.revision = Counter::default();
        }
        values.sort_by(|left, right| left.id.cmp(&right.id));
        values
    }
    fn normalize_history(values: &[HistoryView]) -> Vec<HistoryView> {
        let mut values = values.to_vec();
        for value in &mut values {
            value.revision = Counter::default();
        }
        values.sort_by(|left, right| left.id.cmp(&right.id));
        values
    }
    normalize_backups(current_backups) == normalize_backups(projected_backups)
        && normalize_history(current_history) == normalize_history(projected_history)
}

pub fn project_persisted_views(
    backups: Option<&backup_store::BackupStore>,
    history: Option<&operation_journal::JournalStore>,
    operations: &[dlssync_contracts::OperationSnapshot],
) -> Result<(Vec<BackupView>, Vec<HistoryView>), String> {
    let history_records = history
        .map(operation_journal::JournalStore::list_all)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let mut backup_views = Vec::new();
    if let Some(store) = backups {
        for entry in store.list().map_err(|error| error.to_string())? {
            let kind = if entry.backup_type == "driver_package" {
                BackupKind::DriverPackage
            } else {
                BackupKind::GameDll
            };
            let availability = backup_availability(&entry, kind);
            let verification = store
                .restore_verification(&entry.id)
                .map_err(|error| error.to_string())?;
            let last_restore = verification
                .map(|verification| RestoreVerification {
                    verified: verification.verified,
                    at: verification.at.map(|at| at.to_rfc3339()),
                    detail: verification.detail,
                })
                .or_else(|| {
                    entry.restored_at.map(|at| RestoreVerification {
                        verified: false,
                        at: Some(at.to_rfc3339()),
                        detail: Some(
                            "legacy restore has no recorded destination SHA-256 comparison"
                                .to_string(),
                        ),
                    })
                });
            let operation_id = history_records
                .iter()
                .find(|record| record.backup_id.as_deref() == Some(entry.id.as_str()))
                .map(|record| record.id.clone());
            let operation_in_flight = operations.iter().any(|operation| {
                !operation.stage.is_terminal()
                    && (operation.id == operation_id.as_deref().unwrap_or_default()
                        || operation.game_ids.iter().any(|id| id == &entry.game_id))
            });
            let restore_eligibility = restore_eligibility(
                entry.backup_path.as_os_str().is_empty(),
                availability,
                operation_in_flight,
            );
            backup_views.push(BackupView {
                id: entry.id.clone(),
                game_id: (kind == BackupKind::GameDll).then_some(entry.game_id.clone()),
                component_id: (kind == BackupKind::GameDll)
                    .then(|| format!("{}:{}", entry.game_id, entry.dll_filename)),
                operation_id,
                original_path: entry.original_path.display().to_string(),
                backup_path: entry.backup_path.display().to_string(),
                sha256: entry.previous_sha256.clone(),
                kind,
                availability,
                restore_eligibility,
                last_restore,
                verified_available: availability == BackupAvailability::VerifiedPresent,
                restored_at: entry.restored_at.map(|at| at.to_rfc3339()),
                revision: Counter::default(),
            });
        }
    }
    backup_views.sort_by(|left, right| left.id.cmp(&right.id));

    let source_store_id = history
        .map(operation_journal::JournalStore::source_store_id)
        .transpose()
        .map_err(|error| error.to_string())?;
    let mut history_views = history_records
        .into_iter()
        .map(|record| HistoryView {
            id: format!(
                "history:{}:{}",
                source_store_id.as_deref().unwrap_or("unavailable"),
                record.id
            ),
            source_store_id: source_store_id.clone().unwrap_or_default(),
            source_record_id: record.id.clone(),
            historical_at: Some(record.created_at.clone()),
            operation_id: Some(record.id.clone()),
            game_id: record
                .details
                .get("game_id")
                .cloned()
                .or_else(|| record.target.clone()),
            component_id: record.details.get("component_id").cloned(),
            recovery_outcome: Some(match record.status {
                OperationStatus::Started => OperationStage::Applying,
                OperationStatus::Succeeded if record.kind == OperationKind::Rollback => {
                    OperationStage::RolledBack
                }
                OperationStatus::Succeeded => OperationStage::Completed,
                OperationStatus::Failed => OperationStage::RollbackFailed,
                OperationStatus::Cancelled => OperationStage::Cancelled,
            }),
            record,
            revision: Counter::default(),
        })
        .collect::<Vec<_>>();
    history_views.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((backup_views, history_views))
}

fn backup_availability(entry: &backup_store::BackupEntry, _kind: BackupKind) -> BackupAvailability {
    if !entry.backup_path.exists() {
        return BackupAvailability::VerifiedAbsent;
    }
    let Some(expected) = entry
        .previous_sha256
        .as_deref()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return BackupAvailability::Unverified;
    };
    match dll_catalog::hex_sha256_file(&entry.backup_path) {
        Ok(actual) if actual.eq_ignore_ascii_case(expected) => BackupAvailability::VerifiedPresent,
        Ok(_) | Err(_) => BackupAvailability::Unverified,
    }
}

fn restore_eligibility(
    snapshot_absent: bool,
    availability: BackupAvailability,
    operation_in_flight: bool,
) -> RestoreEligibilityView {
    let reason = if snapshot_absent {
        Some("snapshot_absent")
    } else if availability == BackupAvailability::VerifiedAbsent {
        Some("snapshot_unavailable")
    } else if availability == BackupAvailability::Unverified {
        Some("unverified")
    } else if operation_in_flight {
        Some("operation_in_flight")
    } else if availability != BackupAvailability::VerifiedPresent {
        Some("unspecified")
    } else {
        None
    };
    RestoreEligibilityView {
        eligible: reason.is_none(),
        reason_code: reason.map(str::to_string),
    }
}

fn backup_operation_in_flight(
    backup: &BackupView,
    operations: &[dlssync_contracts::OperationSnapshot],
) -> bool {
    operations.iter().any(|operation| {
        !operation.stage.is_terminal()
            && (backup.operation_id.as_deref() == Some(operation.id.as_str())
                || backup
                    .game_id
                    .as_ref()
                    .is_some_and(|game_id| operation.game_ids.iter().any(|id| id == game_id)))
    })
}

#[cfg(test)]
struct TestClock;

#[cfg(test)]
impl StateClock for TestClock {
    fn now_utc(&self) -> String {
        "2026-09-17T00:00:00Z".to_string()
    }
}

#[cfg(test)]
struct TestPublisher {
    fail: bool,
}

#[cfg(test)]
impl StateEventPublisher for TestPublisher {
    fn publish(&self, _event: &StateEvent) -> Result<(), String> {
        if self.fail {
            Err("forced transport failure".to_string())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dlssync_contracts::{OperationActor, OperationRecord};
    use tempfile::tempdir;

    fn backup_entry(
        store: &backup_store::BackupStore,
        id: &str,
        game_id: &str,
        kind: &str,
        bytes: &[u8],
        with_hash: bool,
    ) -> backup_store::BackupEntry {
        let backup_path = store.root_dir.join(format!("{id}.bin"));
        std::fs::write(&backup_path, bytes).unwrap();
        backup_store::BackupEntry {
            id: id.to_string(),
            game_id: game_id.to_string(),
            dll_family: "dlss_sr".to_string(),
            dll_filename: "nvngx_dlss.dll".to_string(),
            original_path: store.root_dir.join(format!("{id}-installed.dll")),
            backup_path,
            previous_version: Some("1.0.0".to_string()),
            previous_sha256: with_hash.then(|| dll_catalog::hex_sha256(bytes)),
            created_at: chrono::Utc::now(),
            restored_at: None,
            size_bytes: Some(bytes.len() as u64),
            backup_type: kind.to_string(),
            device_class: None,
            hardware_id: None,
            driver_provider: None,
        }
    }

    #[test]
    fn phase4_events_are_unique_consecutive_and_gap_requires_snapshot_repair() {
        let coordinator = StateCoordinator::test("emitter-a");
        let first = coordinator.commit(StateCommit::empty()).unwrap();
        let second = coordinator.commit(StateCommit::empty()).unwrap();
        assert_ne!(first.event.id, second.event.id);
        assert_eq!(first.event.sequence.parse(), Some(1));
        assert_eq!(second.event.sequence.parse(), Some(2));
        assert_eq!(
            validate_next_event(&coordinator.initial_snapshot(), &second.event),
            EventDisposition::Resnapshot
        );
        let repaired = coordinator.snapshot();
        assert_eq!(repaired.sequence.parse(), Some(2));
        assert_eq!(repaired.emitter_id, second.event.emitter_id);
    }

    #[test]
    fn phase4_operation_sequences_are_consecutive_per_emitter() {
        let coordinator = StateCoordinator::test("emitter-a");
        let operation = dlssync_contracts::OperationSnapshot {
            id: "operation-a".to_string(),
            plan_id: "plan-a".to_string(),
            actor: dlssync_contracts::OperationActor::Gui,
            kind: dlssync_contracts::OperationKind::DllApply,
            game_ids: vec!["game-a".to_string()],
            parent_operation_id: None,
            started_at: Some("2026-09-17T00:00:00Z".to_string()),
            sequence: Counter::from(99),
            stage: dlssync_contracts::OperationStage::Applying,
            progress: dlssync_contracts::MeasuredProgress::default(),
            results: Vec::new(),
            cancel_requested: false,
            error: None,
            state_revision: String::new(),
            updated_at: "2026-09-17T00:00:00Z".to_string(),
        };
        let mut first_commit = StateCommit::for_game("game-a");
        first_commit.operation_id = Some(operation.id.clone());
        first_commit.delta.operations.push(operation.clone());
        coordinator.commit(first_commit).unwrap();
        let mut second_commit = StateCommit::for_game("game-a");
        second_commit.operation_id = Some(operation.id.clone());
        second_commit.delta.operations.push(operation);
        coordinator.commit(second_commit).unwrap();
        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.operations[0].sequence.parse(), Some(2));
        assert_eq!(snapshot.sequence.parse(), Some(2));
    }

    #[test]
    fn phase4_stale_observation_cannot_overwrite_newer_revision() {
        let coordinator = StateCoordinator::test("emitter-a");
        let stale = coordinator.begin_observation("game:one");
        let fresh = coordinator.begin_observation("game:one");
        coordinator
            .commit_observation(fresh, StateCommit::for_game("one"))
            .unwrap();
        assert!(matches!(
            coordinator.commit_observation(stale, StateCommit::for_game("one")),
            Err(StateError::StaleObservation { .. })
        ));
        assert_eq!(coordinator.snapshot().revision.parse(), Some(1));
    }

    #[test]
    fn phase4_publication_failure_does_not_change_committed_state() {
        let coordinator = StateCoordinator::test_failing_publisher("emitter-a");
        let receipt = coordinator.commit(StateCommit::empty()).unwrap();
        assert!(receipt.delivery_error.is_some());
        assert_eq!(coordinator.snapshot().revision.parse(), Some(1));
        assert_eq!(coordinator.snapshot().sequence.parse(), Some(1));
    }

    #[test]
    fn phase10_persisted_views_populate_backups_history_and_counts() {
        let root = tempdir().unwrap();
        let backups = backup_store::BackupStore::open(
            root.path().join("backups.db"),
            root.path().join("backups"),
        )
        .unwrap();
        let verified = backup_entry(&backups, "game-backup", "game-a", "dll", b"verified", true);
        let driver = backup_entry(
            &backups,
            "driver-backup",
            "device-a",
            "driver_package",
            b"driver",
            false,
        );
        let unverified = backup_entry(&backups, "legacy-backup", "game-b", "dll", b"legacy", false);
        backups.insert(&verified).unwrap();
        backups.insert(&driver).unwrap();
        backups.insert(&unverified).unwrap();
        let history =
            operation_journal::JournalStore::open(root.path().join("journal.db")).unwrap();
        history
            .append(&OperationRecord {
                id: "apply-a".to_string(),
                created_at: "2026-09-18T00:00:00Z".to_string(),
                actor: OperationActor::Gui,
                kind: OperationKind::DllApply,
                status: OperationStatus::Succeeded,
                target: Some("game-a".to_string()),
                summary: "Applied update".to_string(),
                details: BTreeMap::from([("game_id".to_string(), "game-a".to_string())]),
                duration_ms: Some(1),
                backup_id: Some("game-backup".to_string()),
                error: None,
            })
            .unwrap();

        let coordinator = StateCoordinator::test("emitter-a");
        let receipt = coordinator
            .refresh_persisted_views(Some(&backups), Some(&history))
            .unwrap()
            .expect("persisted state changed");
        let snapshot = coordinator.snapshot();
        assert_eq!(receipt.event.revision, snapshot.revision);
        assert_eq!(snapshot.backups.len(), 3);
        assert_eq!(snapshot.history.len(), 1);
        assert!(snapshot
            .backups
            .iter()
            .all(|backup| backup.revision == snapshot.revision));
        assert!(snapshot
            .history
            .iter()
            .all(|history| history.revision == snapshot.revision));
        assert_eq!(snapshot.counts.protected_games, 1);
        assert_eq!(snapshot.counts.eligible_restorable_backups, 1);
        let legacy = snapshot
            .backups
            .iter()
            .find(|backup| backup.id == "legacy-backup")
            .unwrap();
        assert_eq!(legacy.availability, BackupAvailability::Unverified);
        assert!(!legacy.restore_eligibility.eligible);
        assert_eq!(
            legacy.restore_eligibility.reason_code.as_deref(),
            Some("unverified")
        );
    }

    #[test]
    fn phase10_restore_verification_and_failed_delivery_advance_committed_revision() {
        let root = tempdir().unwrap();
        let backups = backup_store::BackupStore::open(
            root.path().join("backups.db"),
            root.path().join("backups"),
        )
        .unwrap();
        let entry = backup_entry(&backups, "restored", "game-a", "dll", b"verified", true);
        std::fs::write(&entry.original_path, b"installed").unwrap();
        backups.insert(&entry).unwrap();
        assert_eq!(
            crate::transaction::restore_entries(
                &backups,
                std::slice::from_ref(&entry),
                OperationActor::Gui,
            )
            .unwrap(),
            1
        );
        assert_eq!(std::fs::read(&entry.original_path).unwrap(), b"verified");
        let coordinator = StateCoordinator::test_failing_publisher("emitter-a");
        let receipt = coordinator
            .refresh_persisted_views(Some(&backups), None)
            .unwrap()
            .expect("persisted state changed");
        assert!(receipt.delivery_error.is_some());
        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.revision.parse(), Some(1));
        let restored = snapshot
            .backups
            .iter()
            .find(|backup| backup.id == entry.id)
            .unwrap();
        assert!(restored.last_restore.as_ref().unwrap().verified);
        assert!(restored.restore_eligibility.eligible);
        assert_eq!(restored.restore_eligibility.reason_code.as_deref(), None);
    }

    #[test]
    fn phase10_post_apply_observation_advances_revision_for_consumers() {
        let coordinator = StateCoordinator::test("emitter-a");
        let first_ticket = coordinator.begin_observation("game:game-a");
        let mut first = StateCommit::for_game("game-a");
        first.delta.games.push(dlssync_contracts::GameSnapshot {
            id: "game-a".to_string(),
            status: GameStateStatus::UpdateAvailable,
            observation_complete: true,
            ..dlssync_contracts::GameSnapshot::default()
        });
        coordinator.commit_observation(first_ticket, first).unwrap();
        let before_apply_observation = coordinator.snapshot();

        let post_apply_ticket = coordinator.begin_observation("game:game-a");
        let mut post_apply = StateCommit::for_game("game-a");
        post_apply
            .delta
            .games
            .push(dlssync_contracts::GameSnapshot {
                id: "game-a".to_string(),
                status: GameStateStatus::Current,
                observation_complete: true,
                ..dlssync_contracts::GameSnapshot::default()
            });
        let receipt = coordinator
            .commit_observation(post_apply_ticket, post_apply)
            .unwrap();

        assert_eq!(receipt.event.revision.parse(), Some(2));
        assert_eq!(
            validate_next_event(&before_apply_observation, &receipt.event),
            EventDisposition::Apply
        );
        assert_eq!(
            coordinator.snapshot().games[0].status,
            GameStateStatus::Current
        );
    }

    #[test]
    fn phase10_operation_in_flight_updates_backup_eligibility_and_count_atomically() {
        let root = tempdir().unwrap();
        let backups = backup_store::BackupStore::open(
            root.path().join("backups.db"),
            root.path().join("backups"),
        )
        .unwrap();
        let entry = backup_entry(&backups, "game-backup", "game-a", "dll", b"verified", true);
        backups.insert(&entry).unwrap();
        let coordinator = StateCoordinator::test("emitter-a");
        coordinator
            .refresh_persisted_views(Some(&backups), None)
            .unwrap();
        assert_eq!(coordinator.snapshot().counts.eligible_restorable_backups, 1);

        let operation = dlssync_contracts::OperationSnapshot {
            id: "apply-a".to_string(),
            plan_id: "plan-a".to_string(),
            actor: OperationActor::Gui,
            kind: OperationKind::DllApply,
            game_ids: vec!["game-a".to_string()],
            parent_operation_id: None,
            started_at: Some("2026-09-18T00:00:00Z".to_string()),
            sequence: Counter::default(),
            stage: OperationStage::Applying,
            progress: dlssync_contracts::MeasuredProgress::default(),
            results: Vec::new(),
            cancel_requested: false,
            error: None,
            state_revision: String::new(),
            updated_at: "2026-09-18T00:00:00Z".to_string(),
        };
        let mut commit = StateCommit::for_game("game-a");
        commit.delta.operations.push(operation);
        let receipt = coordinator.commit(commit).unwrap();
        let backup_delta = receipt
            .event
            .delta
            .backups
            .iter()
            .find(|backup| backup.id == entry.id)
            .unwrap();
        assert!(!backup_delta.restore_eligibility.eligible);
        assert_eq!(
            backup_delta.restore_eligibility.reason_code.as_deref(),
            Some("operation_in_flight")
        );
        assert_eq!(receipt.event.delta.counts.eligible_restorable_backups, 0);
        assert_eq!(coordinator.snapshot().counts.eligible_restorable_backups, 0);
    }
}
