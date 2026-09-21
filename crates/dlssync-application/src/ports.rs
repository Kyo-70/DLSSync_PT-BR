use dlssync_contracts::{JournalFilter, OperationRecord, StateEvent, UpdatePlan};
use std::path::{Path, PathBuf};

pub trait FileSystemPort {
    type Error;

    fn read(&self, path: &Path) -> Result<Vec<u8>, Self::Error>;
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), Self::Error>;
    fn copy(&self, source: &Path, destination: &Path) -> Result<(), Self::Error>;
}

pub trait JournalPort {
    type Error;

    fn append(&self, record: &OperationRecord) -> Result<(), Self::Error>;
    fn list(&self, filter: &JournalFilter) -> Result<Vec<OperationRecord>, Self::Error>;
}

pub trait BackupPort {
    type Error;

    fn allocate(&self, plan_id: &str, filename: &str) -> Result<PathBuf, Self::Error>;
    fn restore(&self, operation_id: &str) -> Result<(), Self::Error>;
}

pub trait PlatformPort {
    type Error;

    fn verify_authenticode(&self, path: &Path, expected_vendor: &str) -> Result<bool, Self::Error>;
    fn process_is_running_under(&self, root: &Path) -> Result<bool, Self::Error>;
}

pub trait PlanStorePort {
    type Error;

    fn save(&self, plan: &UpdatePlan) -> Result<(), Self::Error>;
    fn load(&self, id: &str) -> Result<UpdatePlan, Self::Error>;
}

pub trait StateClock: Send + Sync {
    fn now_utc(&self) -> String;
}

pub trait StateEventPublisher: Send + Sync {
    fn publish(&self, event: &StateEvent) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct SystemStateClock;

impl StateClock for SystemStateClock {
    fn now_utc(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

#[derive(Debug, Default)]
pub struct NullStateEventPublisher;

impl StateEventPublisher for NullStateEventPublisher {
    fn publish(&self, _event: &StateEvent) -> Result<(), String> {
        Ok(())
    }
}
