pub mod consts;
pub mod sources;
pub mod version;

pub use sources::{DriverRegistry, DriverSource};
pub use version::DriverVersion;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeviceClass {
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DriverVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    Windows10X64,
    Windows11X64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OsTarget {
    pub family: OsFamily,
    pub dch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeviceId {
    pub class: DeviceClass,
    pub vendor: DriverVendor,
    pub pci_vendor_id: u16,
    pub pci_device_id: u16,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Beta,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub struct DriverChangelog {
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub fixed: Vec<String>,
    #[serde(default)]
    pub notes_page_url: Option<String>,
}

impl DriverChangelog {
    pub fn is_empty(&self) -> bool {
        self.highlights.is_empty() && self.fixed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DriverRelease {
    pub vendor: DriverVendor,
    pub version: DriverVersion,
    pub channel: ReleaseChannel,
    #[serde(default)]
    pub display_version: Option<String>,
    #[serde(default)]
    pub is_beta: bool,
    #[serde(default)]
    pub download_url: Option<String>,
    pub size_bytes: u64,
    pub signature_subject: String,
    pub released_at: Option<chrono::DateTime<chrono::Utc>>,
    pub release_notes_url: Option<String>,
    #[serde(default)]
    pub changelog: Option<DriverChangelog>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    Unknown,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DriverHealth {
    Current,
    Outdated,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriverUpdateAction {
    Install {
        download_url: String,
        size_bytes: u64,
    },
    OpenPage {
        url: String,
    },
    None {
        help_url: Option<String>,
    },
}

impl Default for DriverUpdateAction {
    fn default() -> Self {
        Self::None { help_url: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DriverStatusReport {
    pub device: DeviceId,
    pub installed: DriverVersion,
    pub latest: Option<DriverRelease>,
    /// Compatibility projection retained while consumers migrate to `health`.
    pub status: UpdateStatus,
    #[serde(default)]
    pub health: DriverHealth,
    #[serde(default)]
    pub action: DriverUpdateAction,
    #[serde(default)]
    pub reboot_pending: Option<String>,
}

impl DriverStatusReport {
    pub fn new(
        device: DeviceId,
        installed: DriverVersion,
        latest: Option<DriverRelease>,
        status: UpdateStatus,
        reboot_pending: Option<String>,
    ) -> Self {
        let health = driver_health(status);
        let action = driver_update_action(&device, status, latest.as_ref());
        Self {
            device,
            installed,
            latest,
            status,
            health,
            action,
            reboot_pending,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("no driver source for {class:?}/{vendor:?}")]
    NoSource {
        class: DeviceClass,
        vendor: DriverVendor,
    },
}

pub fn update_status(installed: &DriverVersion, latest: Option<&DriverRelease>) -> UpdateStatus {
    match latest {
        None => UpdateStatus::Unknown,
        Some(release) if installed.packed == 0 => {
            let _ = release;
            UpdateStatus::Unknown
        }
        Some(release) if release.version.is_newer_than(installed) => UpdateStatus::UpdateAvailable,
        Some(_) => UpdateStatus::UpToDate,
    }
}

pub fn driver_health(status: UpdateStatus) -> DriverHealth {
    match status {
        UpdateStatus::UpToDate => DriverHealth::Current,
        UpdateStatus::UpdateAvailable => DriverHealth::Outdated,
        UpdateStatus::Unsupported => DriverHealth::Unsupported,
        UpdateStatus::Unknown => DriverHealth::Unknown,
    }
}

pub fn driver_update_action(
    device: &DeviceId,
    status: UpdateStatus,
    latest: Option<&DriverRelease>,
) -> DriverUpdateAction {
    if status != UpdateStatus::UpdateAvailable {
        let help_url = matches!(status, UpdateStatus::Unknown | UpdateStatus::Unsupported)
            .then(|| vendor_help_url(device.vendor).to_string());
        return DriverUpdateAction::None { help_url };
    }

    let Some(release) = latest else {
        return DriverUpdateAction::None { help_url: None };
    };
    if let Some(download_url) = release
        .download_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        return DriverUpdateAction::Install {
            download_url: download_url.to_string(),
            size_bytes: release.size_bytes,
        };
    }
    if let Some(url) = release
        .release_notes_url
        .as_deref()
        .or_else(|| {
            release
                .changelog
                .as_ref()
                .and_then(|changelog| changelog.notes_page_url.as_deref())
        })
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        return DriverUpdateAction::OpenPage {
            url: url.to_string(),
        };
    }

    DriverUpdateAction::None { help_url: None }
}

fn vendor_help_url(vendor: DriverVendor) -> &'static str {
    match vendor {
        DriverVendor::Intel => "https://www.intel.com/content/www/us/en/support/detect.html",
        DriverVendor::Nvidia => "https://www.nvidia.com/Download/index.aspx",
        DriverVendor::Amd => "https://www.amd.com/en/support/download/drivers.html",
        DriverVendor::Other => {
            "https://support.microsoft.com/windows/update-drivers-in-windows-ec62f46c-ff14-c91d-eead-d7126dc1f7b6"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_at(version: DriverVersion) -> DriverRelease {
        DriverRelease {
            vendor: DriverVendor::Nvidia,
            version,
            channel: ReleaseChannel::Stable,
            display_version: None,
            is_beta: false,
            download_url: Some("https://example.test/driver.exe".into()),
            size_bytes: 0,
            signature_subject: "NVIDIA Corporation".into(),
            released_at: None,
            release_notes_url: None,
            changelog: None,
        }
    }

    #[test]
    fn status_is_update_available_when_latest_is_newer() {
        let installed = DriverVersion::nvidia("572.16");
        let latest = release_at(DriverVersion::nvidia("572.83"));
        assert_eq!(
            update_status(&installed, Some(&latest)),
            UpdateStatus::UpdateAvailable
        );
    }

    #[test]
    fn status_is_up_to_date_when_equal_or_older() {
        let installed = DriverVersion::nvidia("572.83");
        let same = release_at(DriverVersion::nvidia("572.83"));
        let older = release_at(DriverVersion::nvidia("566.36"));
        assert_eq!(
            update_status(&installed, Some(&same)),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            update_status(&installed, Some(&older)),
            UpdateStatus::UpToDate
        );
    }

    #[test]
    fn status_is_unknown_without_latest_or_without_installed() {
        let installed = DriverVersion::nvidia("572.16");
        assert_eq!(update_status(&installed, None), UpdateStatus::Unknown);
        let unknown_installed = DriverVersion::unknown();
        let latest = release_at(DriverVersion::nvidia("572.16"));
        assert_eq!(
            update_status(&unknown_installed, Some(&latest)),
            UpdateStatus::Unknown
        );
    }

    #[test]
    fn report_health_preserves_each_authoritative_status() {
        assert_eq!(driver_health(UpdateStatus::UpToDate), DriverHealth::Current);
        assert_eq!(
            driver_health(UpdateStatus::UpdateAvailable),
            DriverHealth::Outdated
        );
        assert_eq!(
            driver_health(UpdateStatus::Unsupported),
            DriverHealth::Unsupported
        );
        assert_eq!(driver_health(UpdateStatus::Unknown), DriverHealth::Unknown);
    }

    #[test]
    fn update_action_prefers_install_then_observed_page_and_fails_closed() {
        let device = DeviceId {
            class: DeviceClass::Gpu,
            vendor: DriverVendor::Nvidia,
            pci_vendor_id: 0x10DE,
            pci_device_id: 0x2705,
            model: "NVIDIA GeForce RTX 4070 Ti SUPER".into(),
        };
        let mut release = release_at(DriverVersion::nvidia("572.83"));
        release.size_bytes = 900;
        assert_eq!(
            driver_update_action(&device, UpdateStatus::UpdateAvailable, Some(&release)),
            DriverUpdateAction::Install {
                download_url: "https://example.test/driver.exe".into(),
                size_bytes: 900,
            }
        );

        release.download_url = None;
        release.release_notes_url = Some("https://example.test/notes".into());
        assert_eq!(
            driver_update_action(&device, UpdateStatus::UpdateAvailable, Some(&release)),
            DriverUpdateAction::OpenPage {
                url: "https://example.test/notes".into(),
            }
        );

        release.release_notes_url = None;
        assert_eq!(
            driver_update_action(&device, UpdateStatus::UpdateAvailable, Some(&release)),
            DriverUpdateAction::None { help_url: None }
        );
    }

    #[test]
    fn unknown_and_unsupported_actions_publish_vendor_help() {
        let device = DeviceId {
            class: DeviceClass::Gpu,
            vendor: DriverVendor::Amd,
            pci_vendor_id: 0x1002,
            pci_device_id: 0x73BF,
            model: "AMD Radeon RX 6900 XT".into(),
        };
        for status in [UpdateStatus::Unknown, UpdateStatus::Unsupported] {
            let DriverUpdateAction::None { help_url } = driver_update_action(&device, status, None)
            else {
                panic!("non-actionable status must not publish an install or page action");
            };
            assert!(help_url
                .as_deref()
                .is_some_and(|url| url.contains("amd.com")));
        }
    }

    #[test]
    fn action_union_serializes_as_a_stable_tagged_contract() {
        let json = serde_json::to_value(DriverUpdateAction::Install {
            download_url: "https://example.test/driver.exe".into(),
            size_bytes: 42,
        })
        .unwrap();
        assert_eq!(json["kind"], "install");
        assert_eq!(json["download_url"], "https://example.test/driver.exe");
        assert_eq!(json["size_bytes"], 42);
    }

    #[test]
    fn legacy_report_payload_deserializes_to_fail_closed_defaults() {
        let device = DeviceId {
            class: DeviceClass::Gpu,
            vendor: DriverVendor::Nvidia,
            pci_vendor_id: 0x10DE,
            pci_device_id: 0x2705,
            model: "NVIDIA GeForce RTX 4070 Ti SUPER".into(),
        };
        let report = DriverStatusReport::new(
            device,
            DriverVersion::nvidia("572.16"),
            Some(release_at(DriverVersion::nvidia("572.83"))),
            UpdateStatus::UpdateAvailable,
            None,
        );
        let mut json = serde_json::to_value(report).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("health");
        object.remove("action");
        object.remove("reboot_pending");

        let legacy: DriverStatusReport = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.health, DriverHealth::Unknown);
        assert_eq!(legacy.action, DriverUpdateAction::None { help_url: None });
        assert_eq!(legacy.reboot_pending, None);
    }
}
