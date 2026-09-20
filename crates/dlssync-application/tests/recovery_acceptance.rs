//! Filesystem acceptance probes: use the same fixtures before and after repair.
use dlssync_application::{build_update_plan, rollback_update_plan};
use dlssync_contracts::UpdatePlanItem;

fn probe(backup_bytes: Option<&[u8]>) {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("game.dll");
    let backup = dir.path().join("backup.dll");
    std::fs::write(&target, b"installed-new").unwrap();
    if let Some(bytes) = backup_bytes {
        std::fs::write(&backup, bytes).unwrap();
    }
    let original = dir.path().join("original.dll");
    std::fs::write(&original, b"original").unwrap();
    let hash = dll_catalog::hex_sha256_file(&original).unwrap();
    let item: UpdatePlanItem = serde_json::from_value(serde_json::json!({
        "id": "component", "game_id": "game", "game_name": "Game",
        "dll_path": target, "family": "dlss_sr", "current_version": null,
        "target_version": "1", "backup_path": backup, "selected": true,
        "trust": {"source_url": "", "expected_sha256": "", "observed_sha256": hash,
            "signature_subject": null, "signature_verified": false, "anti_cheat_risk": null}
    }))
    .unwrap();
    let plan = build_update_plan("2026-09-12T00:00:00Z", vec![item]);
    let result = rollback_update_plan(&plan);
    let bytes = std::fs::read(&target).unwrap();
    println!(
        "backup={} accepted={} target={}",
        if backup_bytes.is_some() {
            "corrupt"
        } else {
            "missing"
        },
        result.is_ok(),
        String::from_utf8_lossy(&bytes)
    );
    assert!(
        result.is_err(),
        "unverified restoration must not report success"
    );
    assert_eq!(
        bytes, b"installed-new",
        "invalid backup must not replace installed bytes"
    );
}

#[test]
fn corrupted_backup_is_rejected_without_writing() {
    probe(Some(b"corrupt"));
}

#[test]
fn missing_backup_is_not_reported_as_success() {
    probe(None);
}
