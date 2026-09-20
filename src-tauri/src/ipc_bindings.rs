pub fn export_typescript_bindings() {
    use dlssync_contracts::*;
    use specta::Types;
    use specta_typescript::Typescript;
    use std::io::Write as _;
    use tauri_specta::LanguageExt;

    let mut types = Types::default()
        .register::<crate::commands::apply::ApplyProgress>()
        .register::<crate::commands::apply::GroupDownloadProgress>()
        .register::<crate::commands::apply::InflightSnapshot>()
        .register::<crate::commands::drivers::InstallProgress>()
        .register::<system_drivers::SystemDriverInstallProgress>()
        .register::<ComponentState>()
        .register::<OperationEvent>()
        .register::<StateEvent>()
        .register::<AuthoritativeSnapshot>()
        .register::<StateWatermark>()
        .register::<DriverInstallPlan>()
        .register::<PlannedChange>()
        .register::<ApplyErrorClass>()
        .register::<DistributionChannel>()
        .register::<InstallMode>()
        .register::<DlssGeneration>()
        .register::<NvidiaGpuArchitecture>()
        .register::<DlssCapability>()
        .register::<CatalogRefreshTrigger>()
        .register::<CatalogDelta>()
        .register::<CatalogProvenance>()
        .register::<CatalogRefreshResult>()
        .register::<CatalogStatus>()
        .register::<OperationActor>()
        .register::<OperationKind>()
        .register::<OperationStatus>()
        .register::<OperationRecord>()
        .register::<JournalFilter>()
        .register::<TrustEvidence>()
        .register::<UpdatePlanItem>()
        .register::<UpdatePlan>()
        .register::<ScannedComponent>()
        .register::<ScannedGame>()
        .register::<ApplyPlanResult>()
        .register::<RollbackPlanResult>()
        .register::<ApiError>();
    let path = std::env::var_os("DLSSYNC_BINDINGS_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../frontend/src/generated/bindings.ts")
        });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("generated bindings directory");
    }
    let functions = specta::function::collect_functions![
        crate::commands::scan::scan_libraries,
        crate::commands::scan::detect_dlls,
        crate::commands::scan::detect_dlss_enabler,
        crate::commands::scan::enrich_game_art,
        crate::commands::scan::fetch_steam_art,
        crate::commands::shell::open_path,
        crate::commands::shell::reveal_path,
        crate::commands::catalog::refresh_catalog,
        crate::commands::catalog::catalog_status,
        crate::commands::catalog::catalog_summary,
        crate::commands::catalog::catalog_latest_shas,
        crate::commands::catalog::list_releases,
        crate::commands::journal::journal_list,
        crate::commands::journal::journal_export,
        // Recipe commands live at `commands/recipes.rs` but are mounted as `recipe_commands`
        // in `lib.rs`, so the collector must use that path to avoid a second module definition.
        crate::local_recipe_commands::list_owned_recipes,
        crate::local_recipe_commands::preview_local_recipe,
        crate::local_recipe_commands::apply_local_recipe,
        crate::local_recipe_commands::configure_local_recipe,
        crate::local_recipe_commands::remove_owned_recipe,
        crate::recipe_commands::list_known_recipes,
        crate::recipe_commands::validate_recipe,
        crate::recipe_commands::preview_recipe_conflicts,
        crate::recipe_commands::remove_recipe,
        crate::commands::apply::apply_update,
        crate::commands::apply::apply_update_batch,
        crate::commands::apply::preview_update_plan,
        crate::commands::apply::cancel_apply,
        crate::commands::apply::cancel_all_applies,
        crate::commands::streamline_set::apply_streamline_set,
        crate::commands::streamline_set::apply_dll_set,
        crate::commands::backup::list_backups,
        crate::commands::backup::restore_backup,
        crate::commands::backup::delete_backup,
        crate::commands::diagnostics::get_log_paths,
        crate::commands::diagnostics::read_recent_logs,
        crate::commands::diagnostics::build_issue_report,
        crate::commands::notifications::list_notifications,
        crate::commands::notifications::mark_notification_read,
        crate::commands::notifications::mark_all_notifications_read,
        crate::commands::notifications::dismiss_notification,
        crate::commands::notifications::push_notification,
        crate::commands::notifications::notifications_unread_count,
        crate::commands::settings::get_settings,
        crate::commands::settings::save_settings,
        crate::commands::settings::add_blacklist_entry,
        crate::commands::settings::remove_blacklist_entry,
        crate::commands::settings::add_favorite_game,
        crate::commands::settings::remove_favorite_game,
        crate::commands::settings::save_window_state,
        crate::commands::settings::get_app_paths,
        crate::commands::advanced::set_dlss_debug_overlay,
        crate::commands::advanced::get_dlss_debug_overlay,
        crate::commands::system::get_system_info,
        crate::commands::drivers::check_driver_updates,
        crate::commands::drivers::list_driver_history,
        crate::commands::drivers::install_driver,
        crate::commands::system_drivers::scan_system_drivers,
        crate::commands::system_drivers::get_system_devices,
        crate::commands::system_drivers::install_system_driver,
        crate::commands::system_drivers::restore_system_driver,
        crate::commands::system_drivers::system_driver_versions,
        crate::commands::anticheat::detect_anticheat,
        crate::commands::dlss_profile::dlss_overrides_supported,
        crate::commands::dlss_profile::dlss_capabilities,
        crate::commands::dlss_profile::apply_dlss_override,
        crate::commands::dlss_profile::reset_dlss_override,
        crate::commands::dlss_profile::read_dlss_override_config,
        crate::commands::dlss_profile::find_game_executable,
        crate::commands::runtime::runtime_mode,
        crate::commands::runtime::open_devtools,
        crate::commands::runtime::state_snapshot,
        crate::commands::runtime::state_watermark,
        crate::commands::background::tray_set_pending,
        crate::commands::ui_prefs::set_efficiency_mode,
        crate::commands::ui_prefs::hide_main_window,
        crate::commands::ui_prefs::show_main_window,
    ](&mut types);
    let mut config = tauri_specta::BuilderConfiguration::default();
    config.commands = functions;
    config.types = types;
    config.error_handling = tauri_specta::ErrorHandlingMode::Throw;
    // Existing IPC uses JSON numbers. New byte counts are decimal strings.
    // Version ordering must use version strings, never the legacy packed field.
    config.dangerously_cast_bigints_to_number = true;
    Typescript::default()
        .export(&config, &path)
        .expect("export contract types");
    let mut output = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open generated bindings");
    output
        .write_all(command_registry_ts(&config.commands).as_bytes())
        .expect("append generated command registry + transport");
    let defaults = serde_json::to_value(crate::commands::settings::AppSettings::default())
        .expect("settings defaults serialize");
    let mut paths = Vec::new();
    required_paths(&defaults, "", &mut paths);
    output
        .write_all(
            format!(
                "\nexport const REQUIRED_SETTINGS_PATHS = {} as const;\n",
                serde_json::to_string_pretty(&paths).unwrap()
            )
            .as_bytes(),
        )
        .expect("append settings shape");
    drop(output);
    let raw = std::fs::read_to_string(&path).expect("read generated bindings");
    let normalized = raw
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{normalized}\n")).expect("normalize generated bindings");
}

fn required_paths(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    if let Some(fields) = value.as_object() {
        for (key, child) in fields {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            out.push(path.clone());
            required_paths(child, &path, out);
        }
    }
}

/// Every Tauri command name, read straight from the `generate_handler!` registry
/// in `lib.rs`, so the projected TypeScript owner can never drift from the actual
/// backend surface. Sorted + deduped for a stable generated file.
fn command_names() -> Vec<String> {
    let lib = include_str!("lib.rs");
    let inner = lib
        .split_once("generate_handler![")
        .and_then(|(_, rest)| rest.split_once("])"))
        .map(|(inner, _)| inner)
        .expect("generate_handler! block present in lib.rs");
    let mut names: Vec<String> = inner
        .split(',')
        .filter_map(|item| item.trim().rsplit("::").next())
        .map(str::trim)
        .filter(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        })
        .map(String::from)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The generated command-name owner (`COMMANDS`) plus the typed transport. Frontend
/// consumers reference `COMMANDS.<name>` instead of hand-written string literals,
/// and `check-architecture` rejects any raw `transport("...")` outside this file.
fn command_registry_ts(functions: &[specta::datatype::Function]) -> String {
    let mut out = String::from("\n\nimport { invoke as tauriInvoke } from \"@tauri-apps/api/core\";\n\nexport const COMMANDS = {\n");
    for name in command_names() {
        out.push_str(&format!("  {name}: \"{name}\",\n"));
    }
    out.push_str("} as const;\n\n");
    out.push_str("export type CommandName = (typeof COMMANDS)[keyof typeof COMMANDS];\n\n");
    out.push_str("type OptionalNullable<T> = { [K in keyof T as null extends T[K] ? never : K]: T[K] } & { [K in keyof T as null extends T[K] ? K : never]?: T[K] };\n");
    out.push_str("export type CommandArguments = {\n");
    for function in functions {
        let name = function.name();
        let method = lower_camel(name);
        out.push_str(&format!("  {name}: OptionalNullable<{{"));
        for (index, (argument, _)) in function.args().iter().enumerate() {
            out.push_str(&format!(
                " {}: Parameters<typeof commands.{method}>[{index}];",
                lower_camel(argument)
            ));
        }
        out.push_str(" }>;\n");
    }
    out.push_str("};\n\nexport type CommandResults = {\n");
    for function in functions {
        let name = function.name();
        let method = lower_camel(name);
        out.push_str(&format!(
            "  {name}: Awaited<ReturnType<typeof commands.{method}>>;\n"
        ));
    }
    out.push_str("};\n\ntype CommandReturn<C extends CommandName> = CommandResults[C] extends null ? void : CommandResults[C];\n");
    out.push_str(
        "export function invokeCommand<C extends CommandName>(command: C, ...[args]: {} extends CommandArguments[C] ? [args?: CommandArguments[C]] : [args: CommandArguments[C]]): Promise<CommandReturn<C>> {\n  return tauriInvoke<CommandReturn<C>>(command, args);\n}\n",
    );
    out
}

fn lower_camel(value: &str) -> String {
    let mut upper = false;
    value
        .chars()
        .filter_map(|ch| {
            if ch == '_' {
                upper = true;
                None
            } else if upper {
                upper = false;
                Some(ch.to_ascii_uppercase())
            } else {
                Some(ch)
            }
        })
        .collect()
}
