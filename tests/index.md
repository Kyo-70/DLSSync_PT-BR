# Test coverage and acceptance reference


- [Local recipe integration](../docs/local-mods.md): local-only recipe actions, durable recovery and explicit unsupported cases.

This page maps the DLSSync v1.7.0 test surface to the evidence it can establish. Use it to choose a test layer without mistaking mocked behavior, successful compilation, or verified file bytes for a working game or installed driver. All paths below are repository-relative unless they describe a generic Windows environment location.

## Scope and verified baseline

The inventory is a static source review as of **2026-09-13**. The following execution results were supplied from that day's verification; they were not rerun while writing this index.

| Command | Verified result as of 2026-09-13 |
|:---|:---|
| `cargo test --workspace --lib` | 15 suites, 555 passing, 1 ignored |
| `pnpm --filter dlssync-frontend exec vitest run` | 67 files, 564 passing, 1 skipped |
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo xtask check-product` | Exit 0 |
| `cargo xtask check-architecture` | Exit 0 |
| `cargo xtask check-bindings` | Exit 0 |
| `cargo xtask check-competitive` | Exit 0 |
| `pnpm --filter dlssync-frontend check` | Exit 0; 0 errors, 0 warnings |
| `pnpm --filter dlssync-frontend lint` | Exit 0 |

The library-only Rust count does **not** include integration-test executables, binary-target tests in `dlssync-cli`, `manifest-builder`, or `xtask`, or documentation tests. The Vitest count does not include Playwright or the dated acceptance scripts. Do not attach either count to `cargo test --workspace`, `task test`, or `pnpm test:e2e`.

The catalogued files are:

| Surface | Test-bearing files | Supporting files |
|:---|---:|:---|
| Vitest unit | 28 | Shared `tests/setup.ts` |
| Vitest integration | 8 | Shared setup |
| Vitest components | 21 | Shared setup; some files inspect source rather than render |
| Vitest contracts | 10 | `tests/contracts/_schema.ts` |
| Playwright WebView2 | 15 specs | 6 configuration, fixture, and helper files |
| Rust under `crates/` | 48 source files containing `#[cfg(test)]`; 11 integration-test files | Integration manifest/resource files noted below |
| Tauri under `src-tauri/` | 19 source files containing `#[cfg(test)]`; 1 integration-test file | Included to explain workspace coverage |
| Dated acceptance under `plans/phase3-continuation-20260912/` | 7 scripts | 4 asserting acceptance scripts, 2 runtime utilities, 1 fixture creator; `live-acceptance.mts` has separate cancel/apply modes |

Copies under `plans/phase3-continuation-20260912/entry/` are historical snapshots, not additional tests selected by Vitest or Cargo.

## Test layer map

Test location alone does not identify the strength of the evidence. A file named “integration” can still use a mocked transport.

| Layer | What it establishes | Automatic execution and boundary |
|:---|:---|:---|
| Pure units and source/schema contracts | Decision logic, serialization shapes, mappings, source wiring, policy invariants | Vitest and Rust test runners; CI runs both. Source-text checks do not execute the checked UI or OS call. |
| Component DOM and frontend store integration | Svelte rendering, interactions, event/store transitions and error handling in happy-dom | Vitest; Tauri, shell, dialogs, events and updater are mocked. This is not real inter-process communication (IPC). |
| Controlled files and services | Temporary-file mutation and rollback, SQLite persistence, synthetic Portable Executable (PE) parsing, local HTTP retry/cancellation, isolated child-process recovery | Non-ignored Rust tests run in CI. Fake service pipelines do not query vendors, install drivers, or launch games. Some library tests probe signed Windows system binaries, with conditional no-op exits described below. |
| Real app UI and IPC with synthetic inventory | The actual debug WebView2 app, navigation, app state, guarded-plan UI, console/exception checks | `pnpm test:e2e` is a separate Windows CI job. It is not part of Vitest or `task test`. Games and GPU reports are fixtures; mutation is not completed by the guarded-plan spec. |
| Real mutation through a running app | Frontend controller dispatch and actual Tauri IPC, file readback, backups, cancellation, stale-plan rejection and coherent multi-file completion | Dated acceptance scripts are **manual**, not wired into CI. They require an owned dev app, pre-existing isolated fixtures, real DLL packages and exclusive access. |
| Real command-line interface (CLI) | Process exit status, JSON envelopes, persisted plan, apply, durable transaction and rollback through `dlssync-cli.exe` | `cli-acceptance.py` is **manual**, not part of Cargo unit tests or CI. It needs a matching binary and isolated real-DLL fixture. |
| External services, hardware and system mutation | Live CDN availability, supplied signed DLL staging, DriverStore export, NVIDIA profile round trips | Explicit opt-in tests; network availability, machine state, privileges or driver mutation make them unsuitable for default deterministic tests. |
| Actual game, installed driver and delivered release | Game startup/rendering, visual correctness, stability, driver activation after reboot, end-user download availability | **Manual acceptance is still required.** No generic test command here establishes these outcomes. Choose and record the actual game, hardware, driver, installation route and public download endpoint. |

### Running the standard layers

Run commands from the repository root, with dependencies installed and the Windows Rust/WebView2 toolchain available for desktop tests. Node and pnpm versions are pinned in `package.json`. **Do not build, test, launch E2E, or stop a runtime while another acceptance owner is using the checkout or its fixture.** Even the E2E build uses the shared frontend build output.

| Layer | Exact command | Expected scope/result |
|:---|:---|:---|
| Frontend once | `pnpm --filter dlssync-frontend exec vitest run` | Verified 564 passing, 1 skipped across 67 files |
| Frontend package alias | `pnpm --filter dlssync-frontend test` | Same configured surface |
| Frontend watch | `pnpm --filter dlssync-frontend test:watch` | Same files, watch mode; not a one-shot acceptance result |
| Rust libraries | `cargo test --workspace --lib` | Verified 555 passing, 1 ignored across 15 suites |
| Full Rust | `cargo test --workspace --no-fail-fast` | Libraries, binaries, integration tests and documentation tests; no verified aggregate count supplied here |
| Standard local aggregate | `task test` | Frontend package test, then `cargo test --workspace`; excludes Playwright and manual harnesses |
| Rust application regressions | `cargo test -p dlssync-application --test planning_catalog_identity --test transaction_profiles --test cli_parity_execution --test recovery_acceptance` | Source-defined default expectation on Windows: 12 passing, 1 ignored; not part of the 555 library count |
| Real WebView2 UI | `pnpm test:e2e` | 15 spec files; no verified pass total supplied; conditional skips below |
| Reuse an existing E2E binary | `DLSS_E2E_SKIP_BUILD=1 pnpm test:e2e` | Skips building only if `target/e2e/debug/dlssync.exe` exists; freshness must be checked separately |
| CLI diagnostic smoke | `cargo run -p dlssync-cli -- --json doctor` | Diagnostic command, not a counted test or proof of apply/rollback |

Run the gate commands in the baseline table separately. Formatting, Clippy, generated-product/binding checks, architecture checks, competitive-claim consistency, type checking and linting establish their respective static contracts, not successful product execution.

CI configuration is in `.github/workflows/ci.yml`. It builds the frontend before Vitest so the bundle budget inspects actual assets, runs full Rust tests on Windows, and then runs WebView2 E2E. Outside an active acceptance session, the bundle prerequisite is `pnpm --filter dlssync-frontend build`; do not treat an old `frontend/dist` as evidence for current source. CI also checks release contracts, dependencies and secrets; those are not extra unit-test pass counts.

## Frontend test files

The 2026-09-19 catalog sweep adds `components/catalogBrowse.test.ts` for vendor-name search, combined filters, empty-state reset and the exact family sent to `listReleases`. `components/catalogNetworkPolicy.test.ts` checks that opening Nexus Catalog does not request vendor drivers. These are rendered component tests with mocked IPC; live visual evidence is recorded separately under `plans/browser-review-20260919/`.

`frontend/vitest.config.ts` selects exactly the four `*.test.ts` directories below and uses happy-dom. `tests/setup.ts` mocks Tauri internals/core/events/app, updater, dialogs and shell; it also supplies animation and media-query stubs. A rendered component therefore proves neither native WebView2 layout nor backend execution.

### Unit files: 28

Paths in this table are relative to `tests/unit/`. “Source” means a text/CSS assertion, not an executed implementation.

| File | Subject and assertions | Does not prove |
|:---|:---|:---|
| `anticheat.test.ts` | Detection helpers, joined names, kernel/tamper/DRM severity, dataset notes, ban versus launch-failure copy | Detection completeness or anti-cheat safety |
| `appNavigation.test.ts` | Back/forward states, detail-rail history, deduplication, forward-history truncation, side-button mapping | Native mouse delivery |
| `applyErrorClass.test.ts` | Error classes, precedence, case folding, retry/action routing, label/tone completeness | Actual recovery from those errors |
| `artifactVersions.test.ts` | Rendered catalog/picker/summary keeps NGX and Streamline artifact versions distinct and avoids equal-version update arrows | Correct PE bytes or an installed update |
| `backgroundAutoApply.test.ts` | Excludes anti-cheat games, probes each game once, fails closed when probing throws | Real protection detection or unattended mutation |
| `backgroundScan.test.ts` | Busy/disabled guards, scan then refresh, tray counts, notification fallback, auto-apply delegation and backend event literals | A real daemon timer, OS toast delivery or apply |
| `brands.test.ts` | Bundled marks, provider normalization, domain fallback, Brandfetch URL construction and labels | Remote logo availability |
| `catalogReleases.test.ts` | Version/hash deduplication, newest-first ordering and packed-u64 rounding regression | Authenticity or availability of a release |
| `community.test.ts` | Support-nudge gating, mocked share fallback, star-count caching and fetch failure | Live sharing or GitHub availability |
| `designTokens.test.ts` | Source spacing/density/rail-width tokens, shared tactile/glass utilities, blur and forced-color scrollbar fallbacks | Pixel appearance, contrast or performance |
| `dlssPresets.test.ts` | SR/RR presets A–O, frame-generation options, source links, active config and DLSS 4/dynamic MFG/DLSS 5 driver thresholds | NVAPI writes or feature support in a game |
| `driversActions.test.ts` | Direct-install versus open-page routing; notes/PDF/help URL fallback | Download or installation success |
| `efficiencyMode.test.ts` | Current/legacy preference precedence, defaults, persistence and change event | Actual Windows EcoQoS behavior |
| `familyMeta.test.ts` | Exhaustive family/vendor/token mappings, consistency and unknown-family handling | Runtime capability on a GPU |
| `favorites.test.ts` | Pure membership/toggle/filter operations, deduplication, order and no mutation | Persistent storage |
| `favoritesToggle.test.ts` | Atomic add/remove API selection, authoritative responses, concurrent different-game toggles | Backend locking over real IPC |
| `formatHuman.test.ts` | Byte/rate/duration/ETA/elapsed/percentage formatting, missing values and numeric boundaries | Transfer accuracy or speed |
| `i18nEngine.test.ts` | Interpolation, pluralization, unknown-key/locale fallback, lazy locale loading and reactive translation | Linguistic quality |
| `i18nParity.test.ts` | Locale key parity, nonempty strings and valid metadata keys | Correct translations |
| `labels.test.ts` | Basenames, vendor/group/catalog/feature mappings, separate FSR/Streamline identities, label completeness | Scanner or planner execution |
| `launcherLogos.test.ts` | Seven launcher definitions, SVG path/background shape and unique order | Launcher discovery |
| `libraryFilters.test.ts` | Technology groups and anti-cheat filtering | Correctness of the supplied scan |
| `notifications.test.ts` | Notification factory defaults, extras/link, unique IDs, ISO time and supported kinds | SQLite persistence or OS delivery |
| `notificationVendor.test.ts` | Structured vendor precedence, safe text fallback and no false NVIDIA branding from “DLSSync” | Correctness of incoming vendor data |
| `relation.test.ts` | Version/hash relationships, targets/pins, library status, preferences, distinct NGX/Streamline schemes and same-major Enabler offers | Binary compatibility or game launch |
| `revertDetect.test.ts` | Latest-backup regression detection, path normalization, restored/driver-package exclusions | Live file monitoring or hash verification |
| `stores-scan-toast.test.ts` | Per-game failure warning, repeated failures, success and unknown-game no-ops | Actual rescan behavior |
| `ux.test.ts` | Fuzzy ranking/highlights, recent commands, shortcuts, command registry, vendor and release URL helpers | Native keyboard delivery or reachable links |

### Integration files: 8

Paths are relative to `tests/integration/`. These integrate frontend functions and stores, not a running Rust backend.

| File | Subject and assertions | Does not prove |
|:---|:---|:---|
| `catalogDiff.test.ts` | Suppresses first-load noise; emits changed existing families only | A live catalog refresh |
| `concurrentApply.test.ts` | Keeps in-flight trackers, merges/prunes completed batches, deduplicates repeated retries, counts backend-added dependencies, normal dispatch without extra confirmation | Real concurrent writes, lock ownership or cancellation |
| `driverInstall.test.ts` | Shared progress survives navigation; one install; idle-event rejection; failure/cancel handling; no empty-URL install; reboot-pending pruning | Driver installation, activation or reboot |
| `driverStatus.test.ts` | Status labels/tones, update counts, ordering and immutable sort | Vendor query correctness |
| `libraryStatus.test.ts` | Mixed-game statuses and outdated-first/alpha ordering | Validity of scanned records |
| `optimisticApply.test.ts` | Immediate optimistic update, Undo, rejection revert, no double revert after Undo | Backend transaction rollback |
| `systemDriverInstall.test.ts` | Shared progress, serialization, reboot hint and visible failed terminal state | Windows Update installation or recovery |
| `toastStore.test.ts` | Unique entries, FIFO stack, fake-timer dismissal and targeted/no-op dismissal | Native notification delivery |

### Component files: 21

Paths are relative to `tests/components/`. Source-contract files are explicitly distinguished from rendered DOM tests.

| File | Subject and assertions | Does not prove |
|:---|:---|:---|
| `ActivityDock.test.ts` | Rendered idle/single/multi-task states, progress, expand action and terminal filtering | Backend progress accuracy |
| `anticheatApplyRisk.test.ts` | Source confirmation/severity/dispatch gates, reset and reduced-motion wiring; EN/ES warning copy | Executed confirmation flow or ban prevention |
| `ApplyProgressModal.test.ts` | Rendered dialog, completed groups, status/version, detail toggle and terminal Dismiss | Completed file mutations |
| `appSideButtonNav.test.ts` | Source history integration, side-button handling, default prevention and listener cleanup | Native mouse events |
| `backgroundSettings.test.ts` | Rendered five toggles, interval choices and mocked persistence | Running background scheduler |
| `BrandMark.test.ts` | SVG/labels/tints, provider resolution, Brandfetch image/fallback rendering | Remote image download |
| `Checkbox.test.ts` | Role/label/state, click toggle and disabled behavior | Screen-reader behavior on Windows |
| `CommandPalette.test.ts` | Open/closed state, groups/chips, filtering/highlighting, arrow navigation and empty state | All dispatched commands |
| `ContextMenu.test.ts` | Menu items, callback, keyboard wrapping, Escape/outside dismissal and viewport clamp | Native display geometry |
| `DlssOverridePanel.test.ts` | SR/FG/RR groups, custom controls, old-driver warning, mocked config hydration and source label | Driver profile reads/writes |
| `DriverHistoryFlyout.test.ts` | Honest WHQL-only toggle and hidden-beta counts | Vendor history completeness |
| `GameDetailDrawer.test.ts` | Source in-flow detail, back/escape, compact hero, sticky action bar, persistent master-detail rail, Enabler/Streamline offer wiring | Rendered layout or successful Streamline update |
| `glassDialogUnification.test.ts` | Source shared floating material/close controls, removed bespoke filters, accent token wiring | Visual consistency on WebView2 |
| `languageSwitcher.test.ts` | Rendered trigger/options/selection and mocked persistence; source root mounting/fixed positioning | Translation quality or native layout |
| `material.test.ts` | Source opaque base surfaces, no base blur/shadow and floating-glass fallback | Measured rendering cost |
| `NotificationsBell.test.ts` | Rendered empty/list/unread states, dismiss controls, badges/vendor logos/links; source root mounting | Persistence, link reachability or OS notification delivery |
| `Select.test.ts` | Selected label, opening listbox and `aria-selected` | Full assistive-technology support |
| `ShortcutOverlay.test.ts` | Closed/open dialog, groups/key chips and close action | Global shortcut interception |
| `streamlineSetUi.test.ts` | Mocked API preserves failed recovery; source set dispatch, trackers/modal, rollback and Enabler offer wiring | Coherent file-set mutation |
| `systemDriverUi.test.ts` | Source driver-package backup grouping/rollback confirmation, admin note/history and snapshot context forwarding | Actual snapshot, install or rollback |
| `Toast.test.ts` | Empty/populated stack, kind/icon, dismiss control and TTL indicator | Wall-clock animation or OS toasts |

### Contract files: 10

Paths are relative to `tests/contracts/`. The schema helper checks a subset of JSON Schema; it is not a Rust-to-TypeScript round-trip runner.

| File | Subject and assertions | Does not prove |
|:---|:---|:---|
| `a11y.test.ts` | Axe critical/serious WCAG-tagged violations for CounterPill, BrandMark, Checkbox and populated Toast | Full-app accessibility; contrast and page-level rules are disabled; moderate/minor findings are not the gate |
| `anticheatReport.test.ts` | Required fields, clean/detected fixtures, rejects unknown detection source/protection kind | Real protection detection |
| `applyErrorCodes.test.ts` | Every emitted stable backend apply code has shipped-locale translations; Streamline preference block is not retryable | Error handling through real IPC |
| `bundleConfig.test.ts` | Publisher/license/homepage, per-user installer, icon/compression, WebView2 bootstrapper and NSIS/MSI targets | A built installer, antivirus reputation or installation |
| `bundleSizeBudget.test.ts` | Existing assets: largest index JS gzip <250 KiB; every index CSS gzip <75 KiB; no unreviewed chunk prefix above 1 KiB gzip | Startup speed, memory, current-source freshness or total bundle budget |
| `driverRelease.test.ts` | Required fields, vendor fixtures and AMD empty-download/open-page model | Vendor endpoint or installer validity |
| `efficiencyMode.test.ts` | Source immediate/repeated EcoQoS wiring, no focus/visibility dependency and matching copy | Task Manager showing efficiency mode |
| `i18nKeyParity.test.ts` | Static translation references, shipped-locale keys and interpolation placeholders | Dynamic keys or translation quality |
| `killScript.test.ts` | Source rejects broad image-name termination and requires repo executable-path matching | Every possible process-ownership scenario |
| `noUnsafeHtml.test.ts` | Source raw-HTML allowlist and no HTML tags in locale strings | Complete injection/security audit |

### Support files under `tests/`

These eight files configure or support tests and are not extra test suites.

| File | Role and limit |
|:---|:---|
| `tests/setup.ts` | Vitest transport/browser stubs described above; cannot prove IPC |
| `tests/contracts/_schema.ts` | Recursive type, required-key, enum, property and array-item assertions; not a full JSON Schema implementation |
| `tests/e2e/config.ts` | Repo/version resolution, binary/build paths, CDP port, timeouts, reports and console-noise allowlist |
| `tests/e2e/playwright.config.ts` | One worker, sequential specs, CI retries, HTML/list reporting and retry traces |
| `tests/e2e/global-setup.ts` | Builds debug/no-bundle app into `target/e2e`; optional existing-binary reuse |
| `tests/e2e/fixtures.ts` | Seeds isolated fictional games, launches owned app, attaches console guard and cleans up its process/profile |
| `tests/e2e/helpers.ts` | View navigation, Backups Activity routing and game-detail opening |
| `tests/e2e/tsconfig.json` | TypeScript configuration for the E2E harness, not a behavioral test |

## Rust test files by crate

The table lists every test-bearing source file under `crates/`, plus all integration-test files. Paths in each row are relative to that crate. Inline modules include both pure tests and temporary-file/service tests; they do not all have the same isolation requirements.

| Crate | Test-bearing files | Coverage and deliberate boundary |
|:---|:---|:---|
| `anticheat-detect` | `src/binaries.rs`, `src/entropy.rs`, `src/lib.rs`, `src/pe_inspect.rs`, `src/signatures.rs` | Temporary directory/depth scans, signature precedence/case folding, entropy vectors, synthetic PE sections/markers and malformed-header resilience. Heuristics do not establish exhaustive detection or safe online play. |
| `backup-store` | `src/lib.rs` | SQLite CRUD, migrations, identifier validation, DLL/driver-package metadata, safe folder names, allocation, deletion and path-prefix migration. Does not itself prove file replacement or Windows driver recovery. |
| `dll-catalog` | `src/download.rs`, `src/hash.rs`, `src/lib.rs`, `src/v3.rs`, `src/zip.rs`; `tests/download_resilience.rs`, `tests/live_endpoint.rs` | Shared download cache/cancellation, digest vectors, Ed25519 exact-byte/fallback/cache verification, source override policy, artifact identity/candidate rejection, anti-cheat index, ZIP identity/traversal/size/hash guards. Local HTTP resilience integration; two opt-in live CDN probes. A valid signature or digest does not imply compatible or downloadable DLLs. |
| `dll-scanner` | `src/lib.rs` | Capped hashes, missing/empty files, case-insensitive families, Enabler markers/ancestor search and distinct FSR/DirectStorage/Streamline identities. Fixture scans do not prove discovery of every real game or a working replacement. |
| `dlssync-application` | `src/execution.rs`, `src/lib.rs`, `src/planning.rs`, `src/policy.rs`, `src/scan.rs`, `src/transaction.rs`; `tests/planning_catalog_identity.rs`, `tests/transaction_profiles.rs`, `tests/cli_parity_execution.rs`, `tests/recovery_acceptance.rs` | Product/distribution/portable policy, fingerprints and drift, dependency expansion, contained backup paths, locks, atomic rollback, preconditions, cancellation, durable recovery, missing/corrupt backups and child-process restart. Integration details below. Does not launch games; shared application API tests do not execute the CLI process. |
| `dlssync-cli` | `src/main.rs` | Unknown hardware fails capability checks, explicit Streamline opt-in and machine-readable policy code. Binary-target tests, not included by `--lib`; no process-level apply/rollback proof. |
| `dlssync-contracts` | `src/lib.rs`, `src/runtime.rs` | Stable snake-case enums, MD5 versus SHA-256 distinction, recovery/cancellation boundaries and lossless byte-count serialization. Does not establish transport delivery. |
| `driver-catalog` | `src/lib.rs`, `src/version.rs`, `src/sources/amd.rs`, `src/sources/intel.rs`, `src/sources/mod.rs`, `src/sources/nvidia.rs`; `tests/pipeline.rs` | Status/version normalization, source registry, vendor parser/history fixtures, PCI/OS/architecture matching and fake-source report pipeline. No live vendor query or driver installation. |
| `driver-install` | `src/download.rs`, `src/launch.rs`, `src/state.rs`, `src/verify.rs`; `tests/download.rs` | Download bounds/cancellation defaults, local HTTP files/retry, installer arguments/Windows quoting, state/progress/exit/reboot mapping, unsigned rejection and conditional signed-system-binary probe. Never runs an installer in these tests. |
| `launcher-scan` | `src/steam.rs` | Steam appmanifest key extraction only. No inline tests in the other launcher parsers; does not prove host Steam/Epic/GOG/EA/Ubisoft/Battle.net/Xbox discovery. |
| `manifest-builder` | `src/main.rs`, `src/download_cache.rs` | Signing/staging/cache integrity, partial-source failure freshness, production x64 asset selection, version/family mappings, anti-cheat distillation, merge/history policy and opt-in supplied signed DLL. Binary-target tests; not a successful production publication or proof that assets can be downloaded. |
| `notifications-store` | `src/lib.rs` | SQLite schema/migrations, all kinds/link/vendor round trips, deduplication, mark/read/dismiss/filter, FIFO eviction, malformed-row handling and concurrent inserts. Not OS notification delivery. |
| `nvapi-drs` | `src/ffi.rs`, `src/settings.rs` | Full-path/basename lookup identity, DRS IDs/values, presets/RR/FG mapping, config round trips and unknown-value fallback. Does not call a live driver in these inline tests. |
| `operation-journal` | `src/lib.rs` | Persisted operations/recovery migration, supersession evidence, filters/export, Windows/POSIX path redaction and backup-linked retention. No proof that an external operation actually succeeded. |
| `pe-version` | `src/authenticode.rs`, `src/identity.rs`, `src/lib.rs`; `tests/malformed_pe.rs` | Version packing, PE architecture/header/offset validation, candidate rejection, subject allowlists/revocation classification, unsigned and conditional live Windows signature probes. No game compatibility or universal parser-fuzzing guarantee. |
| `system-drivers` | `src/classify.rs`, `src/inventory.rs`, `src/lib.rs`, `src/snapshot.rs`, `src/store.rs`, `src/version.rs`; `tests/pipeline.rs`, `tests/snapshot_win.rs` | Device IDs/classification, WMI row parsing, anti-downgrade/dedup/GPU exclusion, version/date comparison, pnputil output/argument parsing, fake inventory/update/install pipeline; opt-in real DriverStore export. No real install, restore point, rollback or reboot proof. |
| `xtask` | `src/main.rs` | Competitive evidence-registry schema, missing evidence/drift rejection, editorial round trips and generated product links. Binary-target tests; does not establish that third-party claims or links remain true live. |

### Rust integration files: 11 under `crates/`

The case totals here are static declarations, not fresh execution results. Platform gates and ignored cases change what runs.

| Repository-relative file | Declared cases | Subject and assertions; boundary |
|:---|---:|:---|
| `crates/dll-catalog/tests/download_resilience.rs` | 13 | Local wiremock HTTP: one fetch for shared XeSS ZIP, byte cap, truncated-body/503 retry, fatal 404, cancellation, compressed/content-length cases, empty response, bad hash, filename case, parallel URLs and progress. No external CDN. |
| `crates/dll-catalog/tests/live_endpoint.rs` | 2 ignored | Live schema-v2/v3 catalogs, required vendors/families/latest metadata; v3 freshness/artifact presence. Does not fetch every referenced DLL. |
| `crates/dlssync-application/tests/planning_catalog_identity.rs` | 3 | Bundled Streamline plan adds distinct common/interposer dependencies; synthetic versioned PE accepts legacy package `1.3.0` with file `1.3.0.7`, rejects attested mismatch/tampering; scanner family/vendor identities stay distinct. No network or proprietary DLL fixture. |
| `crates/dlssync-application/tests/transaction_profiles.rs` | 3 | Disposable child with temporary `LOCALAPPDATA`: pending package blocks another profile; missing-profile takeover fences a returning old journal; manual restore supersedes failed rollback while preserving diagnostics/baselines. Real temporary files/SQLite, not an actual game or power-loss test. |
| `crates/dlssync-application/tests/cli_parity_execution.rs` | 4 default + 1 Windows ignored | Shared application API rejects expanded FSR4 without RDNA4 and forged backup escapes before preparation; backup formula/nested path identity parity. Opt-in NVIDIA case downloads/authenticates/writes one DLL and checks contained original backup and installed digest. Despite the filename, none launches `dlssync-cli.exe`. |
| `crates/dlssync-application/tests/recovery_acceptance.rs` | 2 | Corrupt or missing backup rejects rollback and preserves installed target bytes. Not running-app recovery. |
| `crates/driver-catalog/tests/pipeline.rs` | 5 | Injected fake vendor sources exercise report status, unsupported routing, history deduplication and limit. No external query. |
| `crates/driver-install/tests/download.rs` | 3 | Local server body-to-file, transient 5xx retry and pre-cancel. No installer execution. |
| `crates/pe-version/tests/malformed_pe.rs` | 1 | Malformed optional header returns an error instead of aborting the process. Not exhaustive fuzzing. |
| `crates/system-drivers/tests/pipeline.rs` | 3 | Fake inventory/search/install services exercise safe filtering/grouping, hybrid-GPU exclusion/dedup and progress/success. No live Windows Update install. |
| `crates/system-drivers/tests/snapshot_win.rs` | 1 Windows ignored | Enumerates a real third-party DriverStore package and exports nonempty files into a temp directory. Export only, not restore/install; returns without export if no package exists. |

`crates/driver-install/tests/as-invoker.manifest` and `crates/driver-install/tests/tests.rc` are test-executable resource inputs used by `build.rs`, not additional test cases.

### Tauri workspace tests

The desktop crate's 19 test-bearing source files complement the shared crates. Paths in this table are relative to `src-tauri/`.

| Files | Coverage and boundary |
|:---|:---|
| `src/commands/anticheat.rs` | Manifest signature filtering, local/dataset merge, protection classification and learn-more URL routing; no safety guarantee |
| `src/commands/apply.rs`, `src/commands/apply_decisions.rs` | Expanded policy, recovery diagnostics/events, game-scoped cancellation, running-executable path boundaries, driver normalization, errors/group identity and Streamline preference/major gates; not running IPC |
| `src/commands/catalog.rs` | Added/updated/removed file delta; not live refresh |
| `src/commands/diagnostics.rs` | Encoding/truncation/log-tail/discovery; not complete privacy assurance |
| `src/commands/dlss_profile.rs` | Architecture/driver capability gating; not live NVAPI |
| `src/commands/drivers.rs` | OS/vendor/device normalization, installer filename sanitization and cache clearing; no install |
| `src/commands/notifications.rs` | Event literal, URL allowlist, store layout and push/list/mark/dismiss/eviction; no native toast |
| `src/commands/runtime.rs` | Debug-only DevTools gate; not a release-binary security scan |
| `src/commands/scan.rs` | E2E launcher isolation, custom IDs/path normalization and success/failure journal entries; no complete launcher inventory |
| `src/commands/settings.rs` | UI/background defaults, legacy/partial schema, intervals, round trips and close-to-tray preference; not OS startup behavior |
| `src/commands/streamline_set.rs` | Recovery failure propagation, rollback outcomes, set coherence, RDNA4 gating and backup containment; no live package mutation |
| `src/commands/system_drivers.rs` | WUA IDs/revisions, injection rejection and restore argument quoting; no driver rollback |
| `src/lib.rs` | Background timer periods, cadence changes and missed-tick behavior; not a long-running daemon soak |
| `src/netpolicy.rs` | HTTPS/vendor-host allowlist and malformed/lookalike/userinfo/IP rejection; not endpoint availability |
| `src/paths.rs` | Root/extension/system-directory/symlink guards, data layout, tree movement and idempotent migration; not every filesystem topology |
| `src/state.rs` | Cancellation registry and concurrent singleton/cache coordination; not process-wide UI acceptance |
| `src/system_info.rs` | Memory/GPU/vendor/capability/driver-package classification, deduplication and E2E GPU fixture; not actual hardware performance |
| `src/tray.rs` | Pending-count tooltip; not shell integration |
| `tests/dlss_profile_roundtrip.rs` | Three Windows-only ignored live NVIDIA profile tests: global apply/read/reset, recommended sentinel and privilege-gated FG resilience on synthetic per-game profiles. Mutates driver settings; separate from the 19 source files. |

## Real WebView2 spec files: 15

`tests/e2e/fixtures.ts` starts an owned debug app with `DLSSYNC_E2E=1`, `DLSSYNC_E2E_GPU_FIXTURE=1`, isolated `DLSSYNC_DATA_DIR` and WebView2 user data. Normal profiles use `%LOCALAPPDATA%\Temp\dlssync-e2e-...`; marketing capture uses `%PUBLIC%\DLSSync-...`. It seeds four fictional games, synthetic executables and DLL-named copies of the test app. Those are not runnable games or authentic replacement DLLs.

The default Chrome DevTools Protocol (CDP) port is **9334**, overridable with `DLSSYNC_E2E_CDP_PORT`. The fixture refuses an occupied port instead of attaching to another app, uses one worker and removes its own process/profile afterward. Console guards reject new errors/exceptions per spec and at suite end, except the configured image/CORS/resource allowlist; warnings are collected but not a zero-warning gate.

Paths below are relative to `tests/e2e/`.

| File | Assertions and boundary |
|:---|:---|
| `about.spec.ts` | Version, manifest sources and system-information UI; not live hardware verification |
| `backups.spec.ts` | Hero/search/grouping controls and content-or-empty state; no restore |
| `catalog.spec.ts` | Vendor families/version pickers and embedded DirectStorage entry; no asset download |
| `command-palette.spec.ts` | Open, query and results; not execution of every command |
| `drivers.spec.ts` | GPU vendor UI, overrides panel and bounded system-components scan/admin/history UI; no driver install |
| `game-detail.spec.ts` | Open/back/detail rows, protected fixture warning, confirmation opens Ed25519 review plan; closes plan before mutation |
| `journal.spec.ts` | Filters and persisted startup history; not an apply transaction |
| `library.spec.ts` | Grid/list switch, search/filters and pending-update affordance; no apply-all mutation |
| `marketing-screenshots.spec.ts` | Opt-in capture of eight product surfaces after presence checks; overwrites gallery images, no visual-diff oracle |
| `notifications.spec.ts` | Bell opens notification panel; not OS delivery |
| `settings-daemon.spec.ts` | Master-off dependent-control gating; not background execution |
| `settings.spec.ts` | Version/headings/toggles/tab switching; not persistence across process restart |
| `shell.spec.ts` | Six navigation items and current package version; not installer startup |
| `theme.spec.ts` | Document theme toggles and returns; not contrast certification |
| `zz-console-clean.spec.ts` | Accumulated non-allowlisted console errors and exceptions remain empty; not all warnings or network errors |

## Manual acceptance harnesses

The seven scripts live in `plans/phase3-continuation-20260912/`. They are dated acceptance tools, not portable fixture discovery or general-purpose test runners. The commands below are documentation for a future exclusive run; none should be executed against someone else's current acceptance session.

### Ownership and prerequisites

The app scripts connect to CDP **9333** and look for the Vite page at `http://localhost:1420`. Controller-based scripts discover and import the running app's Vite modules, so a packaged release or E2E binary is not an interchangeable target. They require the owner to have already launched the matching debug app and prepared the documented fixture JSON files.

The generic live profile is `%LOCALAPPDATA%\Temp\dlssync-live-phase3-20260912`; the separate CLI profile is `%LOCALAPPDATA%\Temp\dlssync-cli-phase3-20260912`. These scripts consume fixture metadata containing absolute local paths. `prepare-storage-fixture.py` and `stop-runtime.mts` currently embed a machine-local profile path in source; porting them requires an explicit reviewed path update. Their command lines alone do not make them portable. `inspect-runtime.mts` does not validate profile identity at all, so verify ownership before attaching.

**Two harnesses must never run against the same fixture simultaneously.** Serialize even different fixture games within the shared live profile: page reloads, global cancellation, shared backup counts, trackers and journals can interfere. Do not run `stop-runtime.mts` until the runtime owner has released it. CLI isolation does not authorize a concurrent build of its binary.

### Script map and exact invocations

Run these commands from the repository root using the existing dependencies and matching already-built runtime/binary. `node` executes the `.mts` files with the repository's pinned Node version; Python scripts use `python`.

| Script and command | Fixture/profile | What success establishes and what it does not |
|:---|:---|:---|
| `node plans/phase3-continuation-20260912/live-acceptance.mts cancel` | `plans/phase3-recovery-20260912/one-click-fixture.json`; live profile, `FixtureGames/One Click Lab` | Checks profile/version `1.7.0`, verified baseline (restoring through IPC if needed), real controller dispatch, actual **Cancel all** click, cancelled backend outcomes/terminal UI trackers, no extra confirmation and unchanged SHA-256. Does not prove cancellation at every write boundary. |
| `node plans/phase3-continuation-20260912/live-acceptance.mts apply` | Same one-click fixture/profile | Real apply outcome, changed target, unique backup matching baseline, installed PE-version readback matching reported version, IPC restore with original hash and recorded restoration. Baseline recovery also verifies an undo backup if used. Does not launch a game. |
| `node plans/phase3-continuation-20260912/storage-acceptance.mts` | Local `storage-fixture.json`; live profile, `FixtureGames/DirectStorage Lab` | One requested DirectStorage member expands to two planned files/one dependency, two successful outcomes/trackers, visible `2/2`, no extra dialog, single durable completed set, installed hashes and original backups, then byte-verified restores. Does not prove DirectStorage works in a game. |
| `node plans/phase3-continuation-20260912/review-drift.mts` | `plans/phase3-recovery-20260912/live-fixture.json`; live profile, `FixtureGames/Recovery Lab` | Builds plan through IPC, changes fixture bytes, rejects stale plan, leaves backup count unchanged and no cancellable registration; restores original bytes in `finally` and records hashes. No exhaustive drift/failure matrix. |
| `python plans/phase3-continuation-20260912/cli-acceptance.py` | `plans/phase3-recovery-20260912/cli-fixture.json`; separate CLI profile, `Game` | Launches `target/debug/dlssync-cli.exe --json` for `status`, `plan --path`, `apply --plan --yes`, `rollback --operation --yes`. Asserts zero exits/`ok`, correct data root, one-item plan/apply, changed target, original backup, durable completed record and restored original hash. Does not launch GUI or a game. |
| `node plans/phase3-continuation-20260912/inspect-runtime.mts` | Intended live profile on port 9333; no profile assertion | Dumps body text, optional `__ACCEPTANCE_IPC_TRACE` and time. Diagnostic only: no acceptance assertions or pass count. |
| `node plans/phase3-continuation-20260912/stop-runtime.mts` | Exact live-profile root checked against current machine-local source value | Requests `plugin:process\|exit` after profile check and waits for close. Runtime teardown, not a behavioral test or permission to stop unrelated work. |
| `python plans/phase3-continuation-20260912/prepare-storage-fixture.py` | Live profile; cached catalog and existing One Click Lab executable | Creation-only prerequisite: downloads baseline DirectStorage package `1.0.0`, checks bounded sizes, hashes and x64 DLL PE headers, creates DirectStorage Lab, writes metadata targeting package `1.3.0`. This is not an apply test. |

**`prepare-storage-fixture.py` is creation-only and must never be rerun over an existing fixture.** Its `mkdir(exist_ok=False)` refuses an existing game directory. Do not remove that guard, delete the fixture to force a rerun, or overwrite the JSON baseline to disguise modified bytes. Inspect and recover existing state using verified backups; a failed preparation needs owner review.

`live-acceptance.mts` also supports diagnostic `cancel-dom` and `cancel-ipc` modes. These invoke an element click directly or the cancellation IPC directly; they do not replace the normal `cancel` mode's real Playwright click evidence.

The asserting app scripts write `live-cancel.json`, `live-apply.json`, `live-storage.json` or `review-drift.json`; live modes also capture screenshots. CLI writes `cli-live.json`. Expected success is exit 0 and all assertions satisfied (`passed: true` for the asserting app reports), not a Vitest/Cargo pass count. Reports can be partially written on failure; check the result, identity and readback evidence rather than file existence. Most apply/CLI cleanup is on the success path, not an unconditional recovery guarantee.

## Ignored, skipped and conditionally omitted coverage

The 1 ignored Rust library test and 1 skipped Vitest test in the supplied baseline do not describe all opt-in coverage. The following is the complete explicit ignore/skip inventory in the reviewed test sources.

### Rust ignored tests and opt-in commands

Select individual opt-in suites rather than running the whole workspace with `--ignored`. That would also select an internal crash helper and destructive driver-profile tests.

| File and test | Why ignored | Exact opt-in command |
|:---|:---|:---|
| `crates/dlssync-application/src/transaction.rs`: `crash_child` | Helper intentionally exits a disposable process between writes. Its parent `process_exit_between_members_releases_locks_and_recovers_on_restart` supplies the environment and verifies restart recovery. This is the library baseline's 1 ignored test. | Run the parent: `cargo test -p dlssync-application --lib process_exit_between_members_releases_locks_and_recovers_on_restart`; never invoke the helper standalone |
| `crates/dll-catalog/tests/live_endpoint.rs`: `live_catalog_endpoint_returns_valid_schema_v2`, `live_catalog_endpoint_returns_valid_schema_v3` | Two external jsDelivr CDN requests; network/content can change | `cargo test -p dll-catalog --test live_endpoint -- --ignored` |
| `crates/dlssync-application/tests/cli_parity_execution.rs`: `cli_parity_nvidia_single_file_update_succeeds_with_contained_backup` | Windows-only authentic NVIDIA download and Windows Authenticode validation | `cargo test -p dlssync-application --test cli_parity_execution cli_parity_nvidia_single_file_update_succeeds_with_contained_backup -- --ignored --exact` |
| `crates/manifest-builder/src/main.rs`: `signed_vendor_fixture_remains_signed_after_staging` | Requires supplied signed vendor DLL in `DLSSYNC_SIGNATURE_FIXTURE` | Set that variable to an approved local DLL, then `cargo test -p manifest-builder signed_vendor_fixture_remains_signed_after_staging -- --ignored` |
| `crates/system-drivers/tests/snapshot_win.rs`: `exports_a_real_driverstore_package` | Windows-only live DriverStore access through pnputil | `cargo test -p system-drivers --test snapshot_win -- --ignored` |
| `src-tauri/tests/dlss_profile_roundtrip.rs`: `dlss_overrides_apply_read_reset_round_trip_on_global_profile` | Windows/NVIDIA driver required; destructive global/base-profile apply/read/reset | `cargo test -p dlssync --test dlss_profile_roundtrip dlss_overrides_apply_read_reset_round_trip_on_global_profile -- --ignored --exact` |
| Same file: `recommended_sentinel_round_trips_on_a_synthetic_per_game_profile` | Windows/NVIDIA driver required; writes a synthetic per-game profile, resets afterward | `cargo test -p dlssync --test dlss_profile_roundtrip recommended_sentinel_round_trips_on_a_synthetic_per_game_profile -- --ignored --exact` |
| Same file: `apply_is_resilient_to_privilege_gated_frame_gen_settings` | Windows/NVIDIA driver required; synthetic per-game writes exercise privilege-gated FG settings | `cargo test -p dlssync --test dlss_profile_roundtrip apply_is_resilient_to_privilege_gated_frame_gen_settings -- --ignored --exact` |

There are **9 explicit Rust ignored test functions** in total across these layers, including the helper. Profile reset guards are not evidence that every pre-existing custom driver setting is preserved; obtain approval before live driver tests.

### Frontend and WebView2 skip conditions

These conditions explain missing coverage rather than converting a skipped test into a pass.

| File/test | Skip condition and reason |
|:---|:---|
| `tests/contracts/bundleSizeBudget.test.ts`: dist-existence precondition | `it.runIf(process.env.CI)` runs only when `CI` is truthy. Outside CI it is the supplied baseline's 1 skipped test. |
| Same file: largest index JS, every index CSS, unknown JS chunk prefix | Three budget assertions are under `describe.skipIf(!existsSync(distDir))`; absent `frontend/dist/assets` skips them. CI's precondition must fail instead of silently accepting missing assets. |
| `tests/e2e/marketing-screenshots.spec.ts`: product-surface capture | Skipped unless `DLSSYNC_CAPTURE_MARKETING=1`, because it replaces `.github/assets/nexus/gallery` images. Explicit invocation: `DLSSYNC_CAPTURE_MARKETING=1 pnpm test:e2e marketing-screenshots.spec.ts`. |
| `tests/e2e/library.spec.ts`: grid/list test | No game cards in the library |
| Same file: apply-all affordance test | No games at test time |
| `tests/e2e/game-detail.spec.ts`: open/detail/back test | No games in the library |
| `tests/e2e/drivers.spec.ts`: system-components test | Windows Update scan exceeds the bounded wait, or no outdated component cards exist; these are two branches of one test |

E2E can omit narrower assertions without marking a test skipped: game detail only checks feature/summary rows when rows exist, and driver version history opens only when its toggle exists. Do not interpret those tests as unconditional evidence for those subfeatures. Marketing capture is opt-in even though the CI workflow comment describes zero expected E2E skips.

### Rust probes that can return without the main assertion

These are ordinary test returns, not `#[ignore]`, so the runner can count them as passed without exercising the external dependency.

| File/test | Conditional omission |
|:---|:---|
| `crates/pe-version/src/authenticode.rs`: `live_authenticode_extract_from_signed_dll` | Probes Windows system DLLs. If none exists with an extracted subject, prints a skip diagnostic and returns without the Microsoft-subject assertion. |
| `crates/driver-install/src/verify.rs`: `accepts_microsoft_signed_system_binary` | Windows-only. If no candidate system DLL has an extracted subject, prints a skip diagnostic rather than verifying a signed binary. |
| `crates/system-drivers/tests/snapshot_win.rs`: `exports_a_real_driverstore_package` | Even when explicitly unignored, returns if no third-party package is available to export. |

Windows compile-time gates omit platform-specific tests entirely on other operating systems; `--ignored` cannot enable code that was not compiled. `--lib` similarly excludes binary/integration targets by selection, not by marking them skipped.

## What our tests do NOT prove

Keep the claim at the same level as its evidence. The strongest file/transaction test still does not execute the consumer of the replacement DLL.

- **A correct SHA-256 does not prove a game runs.** It establishes byte identity against the chosen baseline or expected artifact, not rendering correctness, performance, save compatibility, multiplayer safety or stability.
- **A successful vendor query does not prove a driver installed.** Neither parsed metadata, a download, a signature check nor a mocked successful exit proves the intended driver is active after installation and reboot.
- **A compiling build is not a passing run.** Formatting, lint, types, Clippy, generated bindings and an executable's existence cannot substitute for runtime assertions against that exact build.
- **An uploaded asset is not an available download.** Publication/upload evidence does not establish anonymous access, correct public routing, CDN propagation, successful retrieval or matching downloaded bytes.
- **A source contract is not executed UI behavior.** Regex/CSS checks and happy-dom cannot certify WebView2 pixels, keyboard/assistive-technology behavior, high-DPI layouts or runtime performance.
- **A fixture game is not a real game.** Synthetic PE files and DLL-named app copies exercise scanning and UI boundaries; they cannot establish compatibility with an actual title or GPU.
- **A rollback fixture is not every failure mode.** Controlled write failures and process restart tests do not exhaust power loss, disk failure, ACLs, hostile junction changes or antivirus interference.
- **A local report file is not a successful acceptance run.** Verify its outcome, process/profile identity, baseline, installed/backup/restored readbacks and matching source/binary before citing it.

Record actual game launches, hardware and driver installation/readback, reboot results, and public download checks separately. None of those outcomes is implied by the 555 Rust library passes or 564 frontend passes above.

## Current game-dialog and driver workflow checks

- `tests/unit/catalogInstallation.test.ts`: exact family and SHA-256 installation labels; no MD5 or missing-hash inference.
- `tests/components/gameDetailAuthoritativeState.test.ts`: hardware-aware default selection and explicit selections outside the preferred vendor.
- `tests/components/dlssOverrideWriteGate.test.ts`: write gates and feature-specific preset presentation.
- `tests/integration/systemDriverInstall.test.ts`: verified active versions versus pending reboot and failure.
- `crates/system-drivers/src/inventory.rs`: present-device joins without merging distinct interfaces.
- `crates/system-drivers/src/snapshot_manifest.rs`: missing, changed and added backup files.
- `cargo run -p nvapi-drs --example profile_validation`: read-only installed runtime probe. An explicit unique hexadecimal argument enables a temporary unbound-profile persistence test with fresh-session readback and verified cleanup. It does not test game behavior.
- `cargo run -p system-drivers --example device_inventory -- --updates`: real local inventory and a read-only Windows Update query. It does not install drivers.

Live driver installation, reboot and rollback remain separate acceptance actions. Do not run them as incidental tests.

`node --test scripts/build-nexus.test.mjs` checks shell-free command launch and isolated Nexus configuration. Nexus Cargo arguments follow the Tauri separator, and its build-time ACL input omits only the updater permission. A successful strip check does not prove packaged network silence.

Release artifact gate: `cargo xtask verify-updater-signature --installer <exact-standard-installer>` verifies the adjacent Tauri signature against the public key in `src-tauri/tauri.conf.json`. Run it after final signing and before publication.

Windows CI caches E2E executables only under an exact compiler and application-source digest. Test-only selector changes reuse that executable but still run the UI tests. Release packaging never consumes this debug-binary cache.
