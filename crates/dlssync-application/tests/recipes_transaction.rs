use dlssync_application::execution::{ExecutionError, PreparedFile};
use dlssync_application::recipes::{
    apply_recipe_durable, apply_recipe_durable_observed, configure_recipe_durable,
    parse_recipe_json, recover_recipe_transactions, remove_recipe_durable, InstallRule,
    PreparedRecipeMutation, RecipeFilePresence, RecipeMutationKind, RecipeStore,
    RecipeTransactionError, RecipeTransactionStage, RelativePath, RemovalOutcome, Sha256,
};
use operation_journal::JournalStore;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn fixture_recipe() -> dlssync_application::recipes::RecipeV1 {
    parse_recipe_json(include_str!(
        "../../../src-tauri/tests/fixtures/recipe-v1.json"
    ))
    .unwrap()
}

fn present(bytes: &[u8]) -> RecipeFilePresence {
    RecipeFilePresence::Present {
        sha256: Sha256::digest(bytes),
        undo_backup: None,
    }
}

fn staged(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join("staged").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn journal(root: &Path) -> JournalStore {
    JournalStore::open(root.join("operations.sqlite3")).unwrap()
}

fn recipe_for(prepared: &[PreparedRecipeMutation]) -> dlssync_application::recipes::RecipeV1 {
    let mut recipe = fixture_recipe();
    let template = recipe.installed_files[0].clone();
    recipe.installed_files = prepared
        .iter()
        .map(|mutation| {
            let mut file = template.clone();
            file.destination = mutation.relative_path.clone();
            file.archive_entry = mutation.archive_entry.clone();
            file.sha256 = mutation.expected_after.clone();
            file.size_bytes = std::fs::metadata(&mutation.staged).unwrap().len();
            file.install_rule = if mutation.kind == RecipeMutationKind::Replace {
                InstallRule::ReplaceOwned
            } else {
                InstallRule::CreateOnly
            };
            file
        })
        .collect();
    recipe
}

fn created(root: &Path, game: &Path, name: &str, bytes: &[u8]) -> PreparedRecipeMutation {
    PreparedRecipeMutation {
        relative_path: RelativePath::parse(name).unwrap(),
        artifact_id: Some("addon".into()),
        archive_entry: None,
        config: None,
        target: game.join(name),
        staged: staged(root, name, bytes),
        undo_backup: None,
        expected_before: RecipeFilePresence::Absent,
        expected_after: Sha256::digest(bytes),
        kind: RecipeMutationKind::Create,
    }
}

#[test]
fn create_replace_configure_and_remove_restore_the_committed_baseline() {
    let root = tempdir().unwrap();
    let game = root.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let configured = game.join("settings.ini");
    std::fs::write(&configured, b"[mod]\nenabled=false\nother=original\n").unwrap();
    let store = RecipeStore::open_for_install(root.path().join("recipe-store"), &game).unwrap();
    let journal = journal(root.path());
    let first_mutation = created(root.path(), &game, "mod.dll", b"first");
    let mut recipe = recipe_for(std::slice::from_ref(&first_mutation));
    apply_recipe_durable(&store, &journal, "install", &recipe, &[first_mutation]).unwrap();
    let replacement = PreparedRecipeMutation {
        relative_path: RelativePath::parse("mod.dll").unwrap(),
        artifact_id: Some("addon".into()),
        archive_entry: None,
        config: None,
        target: game.join("mod.dll"),
        staged: staged(root.path(), "replacement.dll", b"second"),
        undo_backup: Some(root.path().join("undo/mod.dll")),
        expected_before: present(b"first"),
        expected_after: Sha256::digest(b"second"),
        kind: RecipeMutationKind::Replace,
    };
    recipe = recipe_for(std::slice::from_ref(&replacement));
    recipe.revision = 2;
    recipe.upstream_version = "v2.0.0".into();
    apply_recipe_durable(&store, &journal, "replace", &recipe, &[replacement]).unwrap();
    assert_eq!(std::fs::read(game.join("mod.dll")).unwrap(), b"second");
    recipe.config_keys = serde_json::from_value(serde_json::json!([{
        "path": "settings.ini", "format": "lossless_ini",
        "selector": {"section": "mod", "key": "enabled", "case_rule": "ascii_case_insensitive", "duplicates": "reject"},
        "desired": "true", "expected_before": {"kind": "equals", "value": "false"},
        "source_evidence": "game-test", "preserve_unrelated_bytes": true,
        "remove_rule": "restore_if_value_still_owned"
    }])).unwrap();
    let receipt = configure_recipe_durable(&store, &journal, "configure", &recipe).unwrap();
    assert_eq!(
        std::fs::read(&configured).unwrap(),
        b"[mod]\nenabled=true\nother=original\n"
    );
    std::fs::write(&configured, b"[mod]\nenabled=true\nother=external\n").unwrap();
    let outcome = remove_recipe_durable(&store, &journal, "remove", &recipe.id).unwrap();
    assert!(
        matches!(outcome, RemovalOutcome::Removed { .. }),
        "{outcome:?}"
    );
    assert!(!game.join("mod.dll").exists());
    assert_eq!(
        std::fs::read(&configured).unwrap(),
        b"[mod]\nenabled=false\nother=external\n"
    );
    assert!(matches!(
        store.active_receipt(&recipe.id),
        Err(RecipeTransactionError::MissingActiveReceipt(_))
    ));
    for member in &receipt.members {
        assert!(journal
            .member_superseded(&receipt.transaction_id, &member.target.to_string_lossy())
            .unwrap());
    }
}

#[test]
fn interrupted_write_blocks_a_second_writer_and_recovers_after_reopen() {
    let root = tempdir().unwrap();
    let game = root.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let created = game.join("created.dll");
    let untouched = game.join("untouched.dll");

    let prepared = vec![
        PreparedRecipeMutation {
            relative_path: RelativePath::parse("created.dll").unwrap(),
            artifact_id: Some("addon".into()),
            archive_entry: None,
            config: None,
            target: created.clone(),
            staged: staged(root.path(), "crash-created.dll", b"created"),
            undo_backup: None,
            expected_before: RecipeFilePresence::Absent,
            expected_after: Sha256::digest(b"created"),
            kind: RecipeMutationKind::Create,
        },
        PreparedRecipeMutation {
            relative_path: RelativePath::parse("untouched.dll").unwrap(),
            artifact_id: Some("addon".into()),
            archive_entry: None,
            config: None,
            target: untouched.clone(),
            staged: staged(root.path(), "crash-replace.dll", b"replacement"),
            undo_backup: None,
            expected_before: RecipeFilePresence::Absent,
            expected_after: Sha256::digest(b"replacement"),
            kind: RecipeMutationKind::Create,
        },
    ];
    let store_root = root.path().join("recipe-store");
    let journal_path = root.path().join("operations.sqlite3");
    let store = RecipeStore::open_for_install(&store_root, &game).unwrap();
    let journal = JournalStore::open(journal_path.clone()).unwrap();
    let recipe = recipe_for(&prepared);

    let interrupted = apply_recipe_durable_observed(
        &store,
        &journal,
        "recipe-crash-1",
        &recipe,
        &prepared,
        |index| {
            if index == 0 {
                Err("simulated process exit".into())
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(
        interrupted,
        Err(RecipeTransactionError::Interrupted(_))
    ));
    assert_eq!(std::fs::read(&created).unwrap(), b"created");
    assert!(!untouched.exists());
    let pending = journal.pending_recovery().unwrap();
    assert_eq!(pending.len(), 1);
    let transaction_id = pending[0].id.clone();

    let blocked = apply_recipe_durable(&store, &journal, "recipe-crash-2", &recipe, &prepared);
    assert!(matches!(
        blocked,
        Err(RecipeTransactionError::Execution(ExecutionError::Locked(_)))
    ));
    let normal_apply = dlssync_application::transaction::apply_durable(
        &journal,
        root.path(),
        "normal-apply",
        dlssync_contracts::OperationActor::Gui,
        &[PreparedFile {
            target: created.clone(),
            staged: staged(root.path(), "normal-apply.dll", b"normal apply"),
            backup: root.path().join("normal-apply.backup"),
            previous_sha256: String::new(),
            expected_sha256: Sha256::digest(b"normal apply").to_string(),
            expected_version: None,
        }],
        |_| Ok(()),
    );
    assert!(matches!(normal_apply, Err(ExecutionError::Locked(_))));

    drop(store);
    drop(journal);
    let reopened_store = RecipeStore::open_for_install(&store_root, &game).unwrap();
    let reopened_journal = JournalStore::open(journal_path).unwrap();
    let recovered = recover_recipe_transactions(&reopened_store, &reopened_journal).unwrap();
    assert_eq!(recovered, vec![transaction_id.clone()]);
    assert!(!created.exists());
    assert!(!untouched.exists());
    assert_eq!(
        reopened_store.transaction(&transaction_id).unwrap().0,
        RecipeTransactionStage::RolledBack
    );
    assert!(reopened_journal.pending_recovery().unwrap().is_empty());
}

#[test]
fn newer_receipt_supersedes_old_baseline_and_external_change_is_retained() {
    let root = tempdir().unwrap();
    let game = root.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let target = game.join("mod.dll");
    let store = RecipeStore::open_for_install(root.path().join("recipe-store"), &game).unwrap();
    let journal = journal(root.path());
    let seed = created(root.path(), &game, "mod.dll", b"recipe v1");
    let mut recipe = recipe_for(&[seed]);

    let first = apply_recipe_durable(
        &store,
        &journal,
        "recipe-v1",
        &recipe,
        &[PreparedRecipeMutation {
            relative_path: RelativePath::parse("mod.dll").unwrap(),
            artifact_id: Some("addon".into()),
            archive_entry: None,
            config: None,
            target: target.clone(),
            staged: staged(root.path(), "v1.dll", b"recipe v1"),
            undo_backup: None,
            expected_before: RecipeFilePresence::Absent,
            expected_after: Sha256::digest(b"recipe v1"),
            kind: RecipeMutationKind::Create,
        }],
    )
    .unwrap();

    recipe.revision = 2;
    recipe.upstream_version = "v2.0.0".into();
    recipe.installed_files[0].install_rule = InstallRule::ReplaceOwned;
    recipe.installed_files[0].sha256 = Sha256::digest(b"recipe v2");
    let second = apply_recipe_durable(
        &store,
        &journal,
        "recipe-v2",
        &recipe,
        &[PreparedRecipeMutation {
            relative_path: RelativePath::parse("mod.dll").unwrap(),
            artifact_id: Some("addon".into()),
            archive_entry: None,
            config: None,
            target: target.clone(),
            staged: staged(root.path(), "v2.dll", b"recipe v2"),
            undo_backup: Some(root.path().join("undo/v1.dll")),
            expected_before: present(b"recipe v1"),
            expected_after: Sha256::digest(b"recipe v2"),
            kind: RecipeMutationKind::Replace,
        }],
    )
    .unwrap();
    assert_eq!(second.supersedes_receipt_id, Some(first.receipt_id.clone()));
    assert_eq!(second.members[0].before, RecipeFilePresence::Absent);
    assert!(journal
        .member_superseded(&first.transaction_id, &target.to_string_lossy())
        .unwrap());

    std::fs::write(&target, b"external change").unwrap();
    let outcome = remove_recipe_durable(&store, &journal, "recipe-remove", &recipe.id).unwrap();
    assert!(matches!(outcome, RemovalOutcome::Failed { .. }));
    assert_eq!(std::fs::read(&target).unwrap(), b"external change");
    assert_eq!(store.active_receipt(&recipe.id).unwrap(), second);
    assert!(!journal
        .member_superseded(&second.transaction_id, &target.to_string_lossy())
        .unwrap());
}
