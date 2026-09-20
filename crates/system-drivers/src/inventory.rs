//! Windows device inventory via WMI `Win32_PnPSignedDriver`.

use std::collections::HashMap;

use crate::{classify, DeviceCatalog, DriverError, SystemDevice};

type WmiRow = HashMap<String, wmi::Variant>;

/// Live WMI-backed device catalog.
pub struct WmiInventory;

impl DeviceCatalog for WmiInventory {
    fn inventory(&self) -> Result<Vec<SystemDevice>, DriverError> {
        let rows = query(
            "SELECT DeviceName, DeviceClass, Manufacturer, DriverVersion, DriverDate, DeviceID, \
             InfName FROM Win32_PnPSignedDriver",
        )
        .map_err(DriverError::Inventory)?;
        let entities = query("SELECT DeviceID, Name, PNPClass, Manufacturer, HardwareID, CompatibleID, Present, ConfigManagerErrorCode FROM Win32_PnPEntity")
            .map_err(DriverError::Inventory)?;
        Ok(join_inventory(&rows, &entities))
    }
}

fn query(q: &str) -> Result<Vec<WmiRow>, String> {
    let com = wmi::COMLibrary::new().map_err(|e| format!("COM: {e}"))?;
    let conn = wmi::WMIConnection::new(com).map_err(|e| format!("WMI: {e}"))?;
    conn.raw_query(q).map_err(|e| format!("query: {e}"))
}

/// Map a single WMI row into a [`SystemDevice`]. Rows lacking both a name and a
/// hardware id are skipped (virtual/placeholder entries).
fn device_from_row(row: &WmiRow) -> Option<SystemDevice> {
    let hardware_id = string_field(row, "DeviceID")?.to_ascii_uppercase();
    let name = string_field(row, "DeviceName")
        .or_else(|| string_field(row, "Manufacturer"))
        .unwrap_or_else(|| "Unknown device".to_string());
    let class = classify(&string_field(row, "DeviceClass").unwrap_or_default());
    let manufacturer = string_field(row, "Manufacturer").unwrap_or_default();
    let driver_version = string_field(row, "DriverVersion").filter(|s| !s.is_empty());
    let driver_date = string_field(row, "DriverDate").and_then(|s| cim_to_iso(&s));
    let inf_name = string_field(row, "InfName").map(|s| s.to_ascii_lowercase());

    Some(SystemDevice {
        name,
        class,
        manufacturer,
        driver_version,
        driver_date,
        hardware_id,
        hardware_ids: Vec::new(),
        problem_code: None,
        inf_name,
        present: true,
    })
}

/// Keep each PnP instance intact. Two interfaces can use different driver packages.
fn join_inventory(signed: &[WmiRow], entities: &[WmiRow]) -> Vec<SystemDevice> {
    let drivers: HashMap<_, _> = signed
        .iter()
        .filter_map(device_from_row)
        .map(|device| (device.hardware_id.clone(), device))
        .collect();
    let mut seen = std::collections::HashSet::new();
    entities
        .iter()
        .filter_map(|entity| {
            if !matches!(entity.get("Present"), Some(wmi::Variant::Bool(true))) {
                return None;
            }
            let id = string_field(entity, "DeviceID")?.to_ascii_uppercase();
            if !seen.insert(id.clone()) {
                return None;
            }
            let mut device = drivers.get(&id).cloned().unwrap_or_else(|| SystemDevice {
                name: string_field(entity, "Name").unwrap_or_else(|| id.clone()),
                class: classify(&string_field(entity, "PNPClass").unwrap_or_default()),
                manufacturer: string_field(entity, "Manufacturer").unwrap_or_default(),
                driver_version: None,
                driver_date: None,
                inf_name: None,
                hardware_id: id,
                hardware_ids: Vec::new(),
                problem_code: None,
                present: true,
            });
            device.hardware_ids = ["HardwareID", "CompatibleID"]
                .into_iter()
                .flat_map(|key| match entity.get(key) {
                    Some(wmi::Variant::Array(values)) => values
                        .iter()
                        .filter_map(|value| match value {
                            wmi::Variant::String(id) => Some(id.to_ascii_uppercase()),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect();
            device.problem_code = match entity.get("ConfigManagerErrorCode") {
                Some(wmi::Variant::UI4(code)) => Some(*code),
                Some(wmi::Variant::I4(code)) => u32::try_from(*code).ok(),
                _ => None,
            };
            Some(device)
        })
        .collect()
}

fn string_field(row: &WmiRow, key: &str) -> Option<String> {
    match row.get(key)? {
        wmi::Variant::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Convert a WMI CIM_DATETIME (`yyyymmddHHMMSS.ffffff±UUU`) into ISO
/// `YYYY-MM-DD`. Returns `None` for malformed/empty input.
pub fn cim_to_iso(cim: &str) -> Option<String> {
    let digits: String = cim.chars().take(8).collect();
    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (y, rest) = digits.split_at(4);
    let (m, d) = rest.split_at(2);
    if m == "00" || d == "00" {
        return None;
    }
    Some(format!("{y}-{m}-{d}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_present_instances_without_merging_distinct_interfaces() {
        let entity = |id: &str, present: bool| {
            HashMap::from([
                ("DeviceID".into(), wmi::Variant::String(id.into())),
                (
                    "Name".into(),
                    wmi::Variant::String("Audio interface".into()),
                ),
                ("Present".into(), wmi::Variant::Bool(present)),
                (
                    "HardwareID".into(),
                    wmi::Variant::Array(vec![wmi::Variant::String("SWC\\AUDIO".into())]),
                ),
                ("ConfigManagerErrorCode".into(), wmi::Variant::UI4(28)),
            ])
        };
        let devices = join_inventory(
            &[],
            &[
                entity("SWD\\ONE", true),
                entity("SWD\\TWO", true),
                entity("SWD\\GONE", false),
            ],
        );
        assert_eq!(devices.len(), 2);
        assert_ne!(devices[0].hardware_id, devices[1].hardware_id);
        assert_eq!(devices[0].hardware_ids, vec!["SWC\\AUDIO"]);
        assert_eq!(devices[0].problem_code, Some(28));
        assert_eq!(devices[0].driver_version, None);
    }

    #[test]
    fn parses_cim_datetime() {
        assert_eq!(
            cim_to_iso("20260406000000.000000-000").as_deref(),
            Some("2026-04-06")
        );
        assert_eq!(
            cim_to_iso("20251231235959.999999+000").as_deref(),
            Some("2025-12-31")
        );
        assert_eq!(cim_to_iso("").as_deref(), None);
        assert_eq!(cim_to_iso("garbage").as_deref(), None);
        assert_eq!(cim_to_iso("20260000000000.000000-000").as_deref(), None);
    }

    #[test]
    fn maps_row_to_device() {
        let mut row: WmiRow = HashMap::new();
        row.insert(
            "DeviceID".into(),
            wmi::Variant::String(r"pci\ven_8086&dev_9a49\x".into()),
        );
        row.insert(
            "DeviceName".into(),
            wmi::Variant::String("Intel(R) UHD Graphics".into()),
        );
        row.insert("DeviceClass".into(), wmi::Variant::String("DISPLAY".into()));
        row.insert(
            "Manufacturer".into(),
            wmi::Variant::String("Intel Corporation".into()),
        );
        row.insert(
            "DriverVersion".into(),
            wmi::Variant::String("31.0.101.2141".into()),
        );
        row.insert(
            "DriverDate".into(),
            wmi::Variant::String("20260406000000.000000-000".into()),
        );

        let d = device_from_row(&row).expect("device");
        assert_eq!(d.hardware_id, r"PCI\VEN_8086&DEV_9A49\X");
        assert_eq!(d.class, classify::DeviceClass::Display);
        assert_eq!(d.driver_version.as_deref(), Some("31.0.101.2141"));
        assert_eq!(d.driver_date.as_deref(), Some("2026-04-06"));
    }

    #[test]
    fn skips_row_without_hardware_id() {
        let mut row: WmiRow = HashMap::new();
        row.insert("DeviceName".into(), wmi::Variant::String("Phantom".into()));
        assert!(device_from_row(&row).is_none());
    }
}
