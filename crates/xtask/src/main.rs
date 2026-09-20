use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Debug, Deserialize)]
struct CargoManifest {
    workspace: Workspace,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    package: WorkspacePackage,
}

#[derive(Debug, Deserialize)]
struct WorkspacePackage {
    version: String,
}

#[derive(Debug, Deserialize)]
struct ProductRegistry {
    product: ProductIdentity,
    catalog: ProductCatalog,
    links: ProductLinks,
}

#[derive(Debug, Deserialize)]
struct ProductIdentity {
    name: String,
    repository: String,
    manifest_repository: String,
    nexus: String,
    homepage: String,
}

#[derive(Debug, Deserialize)]
struct ProductCatalog {
    canonical_manifest: String,
    signature_suffix: String,
}

#[derive(Debug, Deserialize)]
struct ProductLinks {
    releases: String,
    releases_latest: String,
    issues: String,
    new_issue: String,
    author: String,
    sponsor: String,
    kofi: String,
    anticheat_faq: String,
}

#[derive(Debug, Deserialize)]
struct CompetitiveRegistry {
    as_of: String,
    corrections: String,
    features: Vec<CompetitiveFeature>,
    products: Vec<CompetitiveProduct>,
    references: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CompetitiveFeature {
    label: String,
    dlssync: CompetitiveCell,
    renderpilot: CompetitiveCell,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompetitiveCell {
    value: String,
    source: String,
    observed_on: String,
    #[serde(default)]
    qualification: String,
}

#[derive(Debug, Deserialize)]
struct CompetitiveProduct {
    name: String,
    release: String,
    column: String,
    repository: String,
    sources: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = workspace_root()?;
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") => set_version(&root, args.next().as_deref()),
        Some("check-bindings") => check_bindings(&root),
        Some("generate-bindings") => generate_bindings(&root),
        Some("generate-product") => generate_product(&root),
        Some("check-product") => check_product(&root),
        Some("check-architecture") => check_architecture(&root),
        Some("generate-competitive") => generate_competitive(&root),
        Some("check-competitive") => check_competitive(&root),
        Some("verify-release") => verify_release(&root, args.collect()),
        _ => Err(
            "usage: cargo xtask <version 1.2.3|generate-bindings|check-bindings|generate-product|check-product|check-architecture|generate-competitive|check-competitive|verify-release --channel standard|nexus|portable>"
                .into(),
        ),
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot resolve workspace root".into())
}

fn workspace_version(root: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(root.join("Cargo.toml")).map_err(display)?;
    toml::from_str::<CargoManifest>(&raw)
        .map(|manifest| manifest.workspace.package.version)
        .map_err(display)
}

fn set_version(root: &Path, version: Option<&str>) -> Result<(), String> {
    let version = version.ok_or_else(|| "version requires a SemVer value".to_string())?;
    validate_semver(version)?;
    replace_version(root.join("Cargo.toml"), "version = \"", version)?;
    for path in [
        root.join("package.json"),
        root.join("frontend/package.json"),
        root.join("src-tauri/tauri.conf.json"),
    ] {
        replace_json_version(path, version)?;
    }
    run_command(root, "cargo", &["check", "--workspace"])?;
    verify_version_surfaces(root, version)
}

fn validate_semver(version: &str) -> Result<(), String> {
    let lanes: Vec<_> = version.split('.').collect();
    if lanes.len() != 3 || lanes.iter().any(|lane| lane.parse::<u64>().is_err()) {
        return Err(format!("invalid release SemVer: {version}"));
    }
    Ok(())
}

fn replace_version(path: PathBuf, prefix: &str, version: &str) -> Result<(), String> {
    let raw = fs::read_to_string(&path).map_err(display)?;
    let start = raw
        .find(prefix)
        .ok_or_else(|| format!("version owner missing in {}", path.display()))?
        + prefix.len();
    let end = raw[start..]
        .find('"')
        .map(|offset| start + offset)
        .ok_or_else(|| format!("version terminator missing in {}", path.display()))?;
    let mut updated = raw;
    updated.replace_range(start..end, version);
    fs::write(path, updated).map_err(display)
}

fn replace_json_version(path: PathBuf, version: &str) -> Result<(), String> {
    let raw = fs::read_to_string(&path).map_err(display)?;
    let old = raw
        .lines()
        .find(|line| line.trim_start().starts_with("\"version\":"))
        .ok_or_else(|| format!("JSON version missing in {}", path.display()))?;
    let indent = old.len() - old.trim_start().len();
    let comma = if old.trim_end().ends_with(',') {
        ","
    } else {
        ""
    };
    let new = format!("{}\"version\": \"{version}\"{comma}", " ".repeat(indent));
    fs::write(&path, raw.replacen(old, &new, 1)).map_err(display)
}

fn verify_version_surfaces(root: &Path, expected: &str) -> Result<(), String> {
    for path in [
        root.join("Cargo.toml"),
        root.join("package.json"),
        root.join("frontend/package.json"),
        root.join("src-tauri/tauri.conf.json"),
    ] {
        let raw = fs::read_to_string(&path).map_err(display)?;
        if !raw.contains(expected) {
            return Err(format!("{} does not contain {expected}", path.display()));
        }
    }
    Ok(())
}

/// Documentation and asset surfaces that state the shipped version to a reader. They drift silently
/// because no compiler reads them, so `check-product` verifies them next to the code surfaces.
/// `Cargo.toml [workspace.package].version` remains the single owner; this only detects divergence.
const DOCUMENTED_VERSION_SURFACES: [&str; 8] = [
    "README.md",
    "llms.txt",
    "docs/index.md",
    "docs/versions-and-compatibility.md",
    "docs/release-marketing.md",
    "docs/nexus-build.md",
    "docs/nexus-description-v1.7.bbcode",
    "tests/index.md",
];

fn verify_documented_version_surfaces(root: &Path, expected: &str) -> Result<(), String> {
    for relative in DOCUMENTED_VERSION_SURFACES {
        let path = root.join(relative);
        let raw =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !raw.contains(expected) {
            return Err(format!(
                "{relative} does not state version {expected}; documentation drifted from Cargo.toml"
            ));
        }
    }
    Ok(())
}

fn generate_bindings(root: &Path) -> Result<(), String> {
    generate_bindings_at(root, &root.join("frontend/src/generated/bindings.ts"))
}

fn generate_bindings_at(root: &Path, output: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .current_dir(root)
        .env("DLSSYNC_BINDINGS_OUTPUT", output)
        .args([
            "run",
            "-p",
            "dlssync",
            "--features",
            "bindings",
            "--example",
            "export_bindings",
        ])
        .status()
        .map_err(display)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("binding generation failed: {status}"))
    }
}

fn check_bindings(root: &Path) -> Result<(), String> {
    let path = root.join("frontend/src/generated/bindings.ts");
    let before = fs::read(&path).map_err(|_| {
        format!(
            "{} is missing; run cargo xtask generate-bindings",
            path.display()
        )
    })?;
    let temporary = tempfile::tempdir().map_err(display)?;
    let output = temporary.path().join("bindings.ts");
    generate_bindings_at(root, &output)?;
    let after = fs::read(&output).map_err(display)?;
    if before != after {
        return Err("generated TypeScript bindings were stale".into());
    }
    Ok(())
}

fn product_registry(root: &Path) -> Result<ProductRegistry, String> {
    let raw = fs::read_to_string(root.join("product.toml")).map_err(display)?;
    toml::from_str(&raw).map_err(display)
}

fn product_typescript(registry: &ProductRegistry) -> Result<String, String> {
    let value = serde_json::json!({
        "name": registry.product.name,
        "repository": registry.product.repository,
        "manifestRepository": registry.product.manifest_repository,
        "nexus": registry.product.nexus,
        "homepage": registry.product.homepage,
        "canonicalManifest": registry.catalog.canonical_manifest,
        "signatureSuffix": registry.catalog.signature_suffix,
        "releases": registry.links.releases,
        "releasesLatest": registry.links.releases_latest,
        "issues": registry.links.issues,
        "newIssue": registry.links.new_issue,
        "author": registry.links.author,
        "sponsor": registry.links.sponsor,
        "kofi": registry.links.kofi,
        "anticheatFaq": registry.links.anticheat_faq,
    });
    let json = serde_json::to_string_pretty(&value).map_err(display)?;
    Ok(format!(
        "// Generated by cargo xtask generate-product. Do not edit.\nexport const PRODUCT = {json} as const;\n"
    ))
}

fn generate_product(root: &Path) -> Result<(), String> {
    let registry = product_registry(root)?;
    let generated = product_typescript(&registry)?;
    fs::write(root.join("frontend/src/generated/product.ts"), generated).map_err(display)?;
    let path = root.join("src-tauri/tauri.conf.json");
    let raw = fs::read_to_string(&path).map_err(display)?;
    let mut config: serde_json::Value = serde_json::from_str(&raw).map_err(display)?;
    config["bundle"]["homepage"] = registry.product.homepage.into();
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&config).map_err(display)?
        ),
    )
    .map_err(display)
}

fn check_product(root: &Path) -> Result<(), String> {
    verify_documented_version_surfaces(root, &workspace_version(root)?)?;
    let registry = product_registry(root)?;
    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("src-tauri/tauri.conf.json")).map_err(display)?,
    )
    .map_err(display)?;
    if config["bundle"]["homepage"].as_str() != Some(registry.product.homepage.as_str()) {
        return Err(
            "Tauri bundle homepage differs from product.toml; run cargo xtask generate-product"
                .into(),
        );
    }
    let expected = product_typescript(&registry)?;
    let path = root.join("frontend/src/generated/product.ts");
    let actual = fs::read_to_string(&path).map_err(|_| {
        format!(
            "{} is missing; run cargo xtask generate-product",
            path.display()
        )
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} is stale; run cargo xtask generate-product",
            path.display()
        ))
    }
}

fn check_architecture(root: &Path) -> Result<(), String> {
    let source = root.join("frontend/src");
    let mut violations = Vec::new();
    visit_files(&source, &mut |path| {
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("ts" | "svelte")
        ) {
            return;
        }
        if path
            .components()
            .any(|part| part.as_os_str() == "generated")
        {
            return;
        }
        if let Ok(raw) = fs::read_to_string(path) {
            for (line, text) in raw.lines().enumerate() {
                if text.contains("invoke(") || text.contains("invoke<") {
                    violations.push(format!("{}:{} raw Tauri invoke", path.display(), line + 1));
                }
                if text.contains("transport(\"")
                    || text.contains("transport('")
                    || text.contains("transport(`")
                    || text.contains("invokeCommand(\"")
                    || text.contains("invokeCommand('")
                    || text.contains("invokeCommand(`")
                {
                    violations.push(format!(
                        "{}:{} raw command string literal — call transport(COMMANDS.<name>) from generated/bindings.ts",
                        path.display(),
                        line + 1
                    ));
                }
                if [
                    "github.com/xt0n1-t3ch",
                    "nexusmods.com/site/mods/1922",
                    "DLSSync-Manifest",
                ]
                .iter()
                .any(|contract| text.contains(contract))
                {
                    violations.push(format!(
                        "{}:{} product URL outside generated/product.ts",
                        path.display(),
                        line + 1
                    ));
                }
            }
        }
    })?;
    if !violations.is_empty() {
        return Err(format!(
            "frontend contract ownership violations:\n{}",
            violations.join("\n")
        ));
    }
    Ok(())
}

fn competitive_markdown(root: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(root.join("data/competitive-products.json")).map_err(display)?;
    let registry: CompetitiveRegistry = serde_json::from_str(&raw).map_err(display)?;
    let path = root.join("docs/competitive-comparison.md");
    let document = fs::read_to_string(path).map_err(display)?;
    render_competitive(root, &registry, &document)
}

fn render_competitive(
    root: &Path,
    registry: &CompetitiveRegistry,
    document: &str,
) -> Result<String, String> {
    if registry.products.len() != 2
        || registry.features.is_empty()
        || !valid_observation_date(&registry.as_of)
        || registry.corrections.trim().is_empty()
    {
        return Err("competitive registry must contain products and features".into());
    }
    for product in &registry.products {
        if product.release.trim().is_empty()
            || product.column.trim().is_empty()
            || product.column.contains(['|', '\n', '\r'])
            || !product.repository.starts_with("https://")
            || product
                .sources
                .iter()
                .any(|source| !source.starts_with("https://"))
        {
            return Err(format!("{} has a non-HTTPS source", product.name));
        }
    }
    if registry.products[0].name != "DLSSync" || registry.products[1].name != "RenderPilot" {
        return Err("competitive product order must be DLSSync, RenderPilot".into());
    }
    validate_competitive_references(root, registry, document)?;
    let mut output = format!(
        "| Capability | {} | {} |\n|---|---|---|\n",
        registry.products[0].column, registry.products[1].column,
    );
    let mut labels = std::collections::BTreeSet::new();
    for feature in &registry.features {
        if feature.label.trim().is_empty()
            || feature.label.contains(['|', '\n', '\r'])
            || !labels.insert(&feature.label)
        {
            return Err("empty, duplicate, or malformed capability label".into());
        }
        let dlssync = cell_markdown(&feature.dlssync, registry)
            .map_err(|error| format!("{} / DLSSync: {error}", feature.label))?;
        let renderpilot = cell_markdown(&feature.renderpilot, registry)
            .map_err(|error| format!("{} / RenderPilot: {error}", feature.label))?;
        output.push_str(&format!(
            "| {} | {dlssync} | {renderpilot} |\n",
            feature.label
        ));
    }
    // Generate only the evidence table; preserve documentation-owned context and references.
    let mut updated = document.replace("\r\n", "\n");
    let range = competitive_matrix_range(&updated)?;
    updated.replace_range(range, &output);
    Ok(updated)
}

fn valid_observation_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let year = value[..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..].parse::<u32>().unwrap_or(0);
    let max_day = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= max_day
}

fn cell_markdown(cell: &CompetitiveCell, registry: &CompetitiveRegistry) -> Result<String, String> {
    if cell.value.trim().is_empty()
        || !valid_observation_date(&cell.observed_on)
        || cell.observed_on > registry.as_of
        || [&cell.value, &cell.source, &cell.qualification]
            .iter()
            .any(|text| text.contains(['|', '\n', '\r']))
    {
        return Err("invalid value, date, or table delimiter".into());
    }
    let source = ["Source: ", "Sources: ", "Source available: "]
        .iter()
        .find_map(|prefix| cell.source.strip_prefix(prefix))
        .filter(|source| !source.trim().is_empty())
        .ok_or_else(|| "cell requires an explicit source".to_string())?;
    let unknown = cell.value == "not verified";
    if !unknown
        && (!cell.qualification.is_empty() || cell.value.to_lowercase().starts_with("not verified"))
    {
        return Err("uncertainty must use the explicit 'not verified' value".into());
    }
    let mut references = 0;
    for part in source.split("][").skip(1) {
        let id = part
            .split_once(']')
            .map(|(id, _)| id)
            .ok_or_else(|| "unterminated source reference".to_string())?;
        if !registry.references.contains_key(id) {
            return Err(format!("undefined source reference [{id}]"));
        }
        if !unknown && id == "registry" {
            return Err("a registry assertion is not implementation evidence".into());
        }
        references += 1;
    }
    if references == 0
        && !(unknown
            && matches!(
                source,
                "no captured implementation evidence"
                    | "no captured recovery implementation evidence"
            ))
    {
        return Err("cell requires source references, or an explicit missing-evidence statement for 'not verified'".into());
    }
    let value = if unknown { "Not verified" } else { &cell.value };
    Ok(format!(
        "{value}{}. {}. Date: {}.",
        cell.qualification, cell.source, cell.observed_on
    ))
}

fn competitive_matrix_range(document: &str) -> Result<std::ops::Range<usize>, String> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    let mut start = None;
    for line in document.split_inclusive('\n') {
        if line.starts_with("| Capability |") {
            if start.is_some() {
                return Err("duplicate matrix header".into());
            }
            start = Some(offset);
        }
        if !line.starts_with('|') {
            if let Some(begin) = start.take() {
                ranges.push(begin..offset);
            }
        }
        offset += line.len();
    }
    if let Some(begin) = start {
        ranges.push(begin..offset);
    }
    if ranges.len() != 1 {
        return Err("document requires exactly one capability matrix".into());
    }
    Ok(ranges.remove(0))
}

fn validate_competitive_references(
    root: &Path,
    registry: &CompetitiveRegistry,
    document: &str,
) -> Result<(), String> {
    let mut actual = std::collections::BTreeMap::new();
    for line in document.lines() {
        if let Some((id, target)) = line
            .strip_prefix('[')
            .and_then(|line| line.split_once("]: "))
        {
            if actual.insert(id, target).is_some() {
                return Err(format!("duplicate reference [{id}]"));
            }
        }
    }
    for (id, target) in &registry.references {
        if actual.get(id.as_str()).copied() != Some(target.as_str()) {
            return Err(format!("document source [{id}] disagrees with registry"));
        }
        if target.trim().is_empty() || target.contains(['\n', '\r']) {
            return Err(format!("empty or malformed reference [{id}]"));
        }
        // Check local evidence existence, never fetch external URLs.
        if !target.starts_with("https://") && !root.join("docs").join(target).is_file() {
            return Err(format!("local evidence [{id}] is missing: {target}"));
        }
    }
    Ok(())
}

fn generate_competitive(root: &Path) -> Result<(), String> {
    fs::write(
        root.join("docs/competitive-comparison.md"),
        competitive_markdown(root)?,
    )
    .map_err(display)
}

fn check_competitive(root: &Path) -> Result<(), String> {
    let expected = competitive_markdown(root)?;
    let path = root.join("docs/competitive-comparison.md");
    let actual = fs::read_to_string(&path).map_err(display)?;
    if actual.replace("\r\n", "\n") == expected {
        println!("Competitive evidence matrix verified");
        Ok(())
    } else {
        Err(format!(
            "{} evidence matrix disagrees with data/competitive-products.json",
            path.display()
        ))
    }
}

fn verify_release(root: &Path, args: Vec<String>) -> Result<(), String> {
    check_product(root)?;
    let channel = args
        .windows(2)
        .find(|pair| pair[0] == "--channel")
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| "verify-release requires --channel".to_string())?;
    if !matches!(channel, "standard" | "nexus" | "portable") {
        return Err(format!("unknown release channel: {channel}"));
    }
    let version = workspace_version(root)?;
    verify_version_surfaces(root, &version)?;
    let product = fs::read_to_string(root.join("product.toml")).map_err(display)?;
    if !product.contains(&format!("[distribution.{channel}]")) {
        return Err(format!("product.toml has no {channel} distribution policy"));
    }
    if channel == "nexus" {
        let cargo = fs::read_to_string(root.join("src-tauri/Cargo.toml")).map_err(display)?;
        if !cargo.contains("nexus =")
            || !cargo.contains("standard = [\"dep:tauri-plugin-updater\"]")
            || !cargo.contains("tauri-plugin-updater = { version = \"2\", optional = true }")
        {
            return Err("Nexus must exclude the optional standard-only updater dependency".into());
        }
        let generated = root.join("target/channels/nexus/config");
        let config = fs::read_to_string(generated.join("tauri.conf.json")).map_err(|_| {
            "target/channels/nexus/config/tauri.conf.json missing; run pnpm run check:nexus first"
                .to_string()
        })?;
        let capability = fs::read_to_string(generated.join("default.capability.json")).map_err(
            |_| {
                "target/channels/nexus/config/default.capability.json missing; run pnpm run check:nexus first"
                    .to_string()
            },
        )?;
        // This is a merge-patch overlay. Explicit null removes the Standard plugin;
        // omitting the key would inherit it from the base configuration.
        let config_value: serde_json::Value = serde_json::from_str(&config).map_err(display)?;
        if config_value.pointer("/plugins/updater") != Some(&serde_json::Value::Null)
            || config.contains("latest.json")
            || config_value.pointer("/build/frontendDist")
                != Some(&serde_json::Value::String("../frontend/dist-nexus".into()))
        {
            return Err("packaged Nexus config still exposes updater capability".into());
        }
        if capability.contains("updater:default") {
            return Err("packaged Nexus capability still grants updater permission".into());
        }
        let lib = fs::read_to_string(root.join("src-tauri/src/lib.rs")).map_err(display)?;
        if !lib.contains("#[cfg(feature = \"standard\")]") {
            return Err("Nexus binary lacks compile-time updater exclusion proof".into());
        }
    }
    if channel == "portable" {
        let workflow =
            fs::read_to_string(root.join(".github/workflows/release.yml")).map_err(display)?;
        if !workflow.contains("portable.flag") || !workflow.contains("under .\\data") {
            return Err(
                "portable archive must carry portable.flag and local-data documentation".into(),
            );
        }
    }
    println!("DLSSync {version} {channel} release contract verified");
    Ok(())
}

fn run_command(root: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(display)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed with {status}", args.join(" ")))
    }
}

fn visit_files(root: &Path, visit: &mut impl FnMut(&Path)) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(display)? {
        let entry = entry.map_err(display)?;
        let path = entry.path();
        if path.is_dir() {
            visit_files(&path, visit)?;
        } else {
            visit(&path);
        }
    }
    Ok(())
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod competitive_tests {
    use super::*;

    #[test]
    fn documented_version_surfaces_detect_drift_and_accept_the_workspace_version() {
        let root = workspace_root().unwrap();
        let version = workspace_version(&root).unwrap();
        verify_documented_version_surfaces(&root, &version).unwrap();

        let temp = tempfile::tempdir().unwrap();
        for relative in DOCUMENTED_VERSION_SURFACES {
            let destination = temp.path().join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            // Every surface states the real version except the one under test, which states a
            // different one. A drifted surface must fail, not pass silently.
            let body = if relative == "README.md" {
                "DLSSync 9.9.9\n".to_string()
            } else {
                format!("DLSSync {version}\n")
            };
            fs::write(&destination, body).unwrap();
        }
        let drifted = verify_documented_version_surfaces(temp.path(), &version);
        assert!(
            drifted.is_err(),
            "a drifted documentation surface must fail"
        );
        let message = drifted.unwrap_err();
        assert!(
            message.contains("README.md") && message.contains(&version),
            "the error must name the surface and the expected version: {message}"
        );
    }

    fn fixture() -> (PathBuf, CompetitiveRegistry, String) {
        let root = workspace_root().unwrap();
        let registry =
            serde_json::from_str(include_str!("../../../data/competitive-products.json")).unwrap();
        let document = fs::read_to_string(root.join("docs/competitive-comparison.md")).unwrap();
        (root, registry, document)
    }

    #[test]
    fn reviewed_matrix_round_trips_and_preserves_editorial_context() {
        let (root, registry, document) = fixture();
        assert_eq!(
            render_competitive(&root, &registry, &document).unwrap(),
            document.replace("\r\n", "\n")
        );
        let edited = format!("Editorial preface\n{document}\nEditorial conclusion\n");
        assert_eq!(
            render_competitive(&root, &registry, &edited).unwrap(),
            edited.replace("\r\n", "\n")
        );
    }

    #[test]
    fn missing_required_fields_and_legacy_values_fail_deserialization() {
        for raw in [
            r#"{"value":"yes","observed_on":"2026-09-13"}"#,
            r#"{"value":"yes","source":"Source: [scanner][scanner]"}"#,
            r#"true"#,
            r#""yes""#,
        ] {
            assert!(serde_json::from_str::<CompetitiveCell>(raw).is_err());
        }
    }

    #[test]
    fn empty_missing_and_invalid_evidence_fail() {
        let (root, mut registry, document) = fixture();
        for source in [
            "",
            "Source: ",
            "Source: [missing][missing]",
            "Source: [registry][registry]",
            "Source: no captured implementation evidence",
        ] {
            registry.features[0].dlssync.source = source.into();
            assert!(
                render_competitive(&root, &registry, &document).is_err(),
                "{source}"
            );
        }
        let (_, mut registry, _) = fixture();
        for date in ["", "2026-02-30", "2026-9-13", "2026-09-14", "0000-01-01"] {
            registry.features[0].dlssync.observed_on = date.into();
            assert!(
                render_competitive(&root, &registry, &document).is_err(),
                "{date}"
            );
        }
        assert!(valid_observation_date("2024-02-29"));
        assert!(!valid_observation_date("2025-02-29"));
    }

    #[test]
    fn document_cell_and_registry_cell_drift_are_detected() {
        let (root, mut registry, document) = fixture();
        for (from, to) in [
            ("CLI adapter exists.", "No."),
            ("Source: [CLI entrypoint][cli].", ""),
            ("Date: 2026-09-13.", "Date: 2026-09-12."),
        ] {
            let changed = document.replacen(from, to, 1);
            assert_ne!(
                render_competitive(&root, &registry, &changed).unwrap(),
                changed.replace("\r\n", "\n")
            );
        }
        registry.features[0].dlssync.value = "Changed claim".into();
        assert_ne!(
            render_competitive(&root, &registry, &document).unwrap(),
            document.replace("\r\n", "\n")
        );
    }

    #[test]
    fn source_target_drift_missing_files_and_duplicate_matrices_fail() {
        let (root, mut registry, document) = fixture();
        let changed = document.replace(
            "[cli]: ../crates/dlssync-cli/src/main.rs",
            "[cli]: missing.rs",
        );
        assert!(render_competitive(&root, &registry, &changed).is_err());
        registry
            .references
            .insert("cli".into(), "missing.rs".into());
        assert!(render_competitive(&root, &registry, &changed).is_err());
        assert!(
            competitive_matrix_range(&format!("{document}\n| Capability | duplicate |\n")).is_err()
        );
        assert!(competitive_matrix_range("no table").is_err());
    }

    #[test]
    fn uncertainty_counts_and_missing_evidence_remain_explicit() {
        let (_, registry, _) = fixture();
        let cells: Vec<_> = registry
            .features
            .iter()
            .flat_map(|f| [&f.dlssync, &f.renderpilot])
            .collect();
        assert_eq!(cells.len(), 26);
        assert_eq!(
            cells.iter().filter(|c| c.value == "not verified").count(),
            15
        );
        assert!(registry
            .features
            .iter()
            .all(|f| f.renderpilot.value == "not verified"));
    }
}

#[cfg(test)]
mod product_generation_tests {
    use super::*;

    #[test]
    fn generated_product_contract_contains_every_public_link() {
        let registry: ProductRegistry = toml::from_str(
            r#"
                [product]
                name = "DLSSync"
                repository = "https://example.test/app"
                manifest_repository = "https://example.test/manifest"
                nexus = "https://example.test/nexus"
                homepage = "https://example.test/home"

                [catalog]
                canonical_manifest = "https://example.test/manifest.json"
                signature_suffix = ".sig"

                [links]
                releases = "https://example.test/releases"
                releases_latest = "https://example.test/releases/latest"
                issues = "https://example.test/issues"
                new_issue = "https://example.test/issues/new"
                author = "https://example.test/author"
                sponsor = "https://example.test/sponsor"
                kofi = "https://example.test/kofi"
                anticheat_faq = "https://example.test/anticheat"
            "#,
        )
        .unwrap();

        let generated = product_typescript(&registry).unwrap();

        assert!(generated.starts_with("// Generated by cargo xtask generate-product."));
        assert!(generated.contains("\"repository\": \"https://example.test/app\""));
        assert!(generated.contains("\"canonicalManifest\": \"https://example.test/manifest.json\""));
        assert!(generated.contains("\"anticheatFaq\": \"https://example.test/anticheat\""));
    }
}
