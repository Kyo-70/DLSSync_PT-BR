# Compare documented updater capabilities

This matrix records repository evidence reviewed on **2026-09-13**, not a benchmark or a ranking. Each product cell includes its observation date and source. “Not verified” means the available evidence is insufficient; it does not mean a feature is absent.

DLSSync's column describes the local 1.7.0 development source, not a certified public release. RenderPilot's older local registry names 1.4.1, but no competitor source snapshot or executable was inspected for this review. Its registry assertions are therefore not promoted to Yes/No claims. Other projects previously named in README, including DLSS Swapper, DLSS Updater, Recol, DLSS Enabler and OptiScaler, are not evaluated here; unsupported competitive and coexistence claims were removed.

## Evidence matrix

| Capability | DLSSync, local 1.7.0 source | RenderPilot, version not independently verified |
|---|---|---|
| DLSS / FSR / XeSS / DirectStorage recognition | Implemented filename/family mapping; not universal game coverage. Source: [scanner][scanner]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient implementation evidence. Date: 2026-09-13. |
| Streamline matched-set workflow | Set command and validation exist; runtime outcomes not tested here. Source: [set adapter][sets]. Date: 2026-09-13. | Not verified. Source: no captured implementation evidence. Date: 2026-09-13. |
| CLI | CLI adapter exists. Source: [CLI entrypoint][cli]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient implementation evidence. Date: 2026-09-13. |
| Operation journal | Durable journal implementation exists. Source: [journal][journal]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient implementation evidence. Date: 2026-09-13. |
| GPU driver updates | Vendor resolution and installer orchestration exist; no hardware test here. Sources: [driver catalog][drivers], [installer][installer]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], not proof of absence. Date: 2026-09-13. |
| Windows device-driver updates | WUA path exists. Source: [Windows Update Agent][wua]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], not proof of absence. Date: 2026-09-13. |
| Detached catalog signature | Ed25519 verification is implemented. Source: [catalog][catalog]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], not proof of absence. Date: 2026-09-13. |
| Artifact hash and publisher checks | Per-entry hashes include historical MD5; publisher checks are separate. Do not generalize strict service checks to every advanced/legacy configuration. Sources: [hash algorithms][hash], [execution][execution]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient trust-path evidence. Date: 2026-09-13. |
| Local backup restore | SHA-256-verified snapshots and restore implementation exist; restore can fail. Sources: [backup index][backups], [verified recovery][recovery]. Date: 2026-09-13. | Not verified. Source: no captured recovery implementation evidence. Date: 2026-09-13. |
| No binary hosting in separate catalog repository | Not verified as an exhaustive external-repository claim. Source available: [catalog source metadata][catalog], not a remote hosting inventory. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], not a hosting inventory. Date: 2026-09-13. |
| RenoDX / Luma installation | Not verified as an exhaustive presence/absence claim. Source available: [local registry assertion][registry], insufficient feature proof. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient implementation evidence. Date: 2026-09-13. |
| Nexus channel | Product policy exists; manual-only network and packaging gaps remain. Sources: [product policy][product], [channel review][nexus]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], not publication evidence. Date: 2026-09-13. |
| GUI update preparation and source details | Internal preparation and optional detail surfaces exist; no routine review requirement implied. Sources: [controller][controller], [update detail widget][widget]. Date: 2026-09-13. | Not verified. Source available: [local registry assertion][registry], insufficient UI evidence. Date: 2026-09-13. |

## Update this comparison responsibly

Record the exact release/commit or inspected artifact, source location, observation date and what was actually checked before replacing “not verified.” A homepage link or an untested local Yes/No registry is not implementation proof. A missing search result is not proof of absence.

This page replaces the former generated matrix. `data/competitive-products.json` and the `cargo xtask generate-competitive` generator remain outside this documentation edit: regenerating from the current registry would reintroduce unsupported Yes/No cells. The generator/registry owner must preserve per-cell evidence and uncertainty before using generation again. CI currently runs `cargo xtask check-competitive`, whose byte-for-byte comparison against the old generator will reject this replacement as stale. That source-level conflict is unresolved because the generator, registry and CI are outside this documentation task's edit scope; the command was not run here. No competitor sites, artifacts or releases were fetched or executed for this review.

[scanner]: ../crates/dll-scanner/src/lib.rs
[sets]: ../src-tauri/src/commands/streamline_set.rs
[cli]: ../crates/dlssync-cli/src/main.rs
[journal]: ../crates/operation-journal/src/lib.rs
[drivers]: ../crates/driver-catalog/src/lib.rs
[installer]: ../crates/driver-install/src/lib.rs
[wua]: ../crates/system-drivers/src/wua.rs
[catalog]: ../crates/dll-catalog/src/lib.rs
[hash]: ../crates/dll-catalog/src/hash.rs
[execution]: ../crates/dlssync-application/src/execution.rs
[backups]: ../crates/backup-store/src/lib.rs
[recovery]: ../crates/dlssync-application/src/transaction.rs
[product]: ../product.toml
[nexus]: nexus-build.md
[controller]: ../frontend/src/lib/applyController.ts
[widget]: ../frontend/src/widgets/UpdatePlanModal.svelte
[registry]: ../data/competitive-products.json
