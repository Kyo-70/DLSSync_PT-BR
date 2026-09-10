# Runtime contracts

Rust owns observed file and driver state, compatibility, update plans, execution,
and recovery. The frontend owns navigation, presentation, and temporary selection.

## Owners

| Domain | Owner | Boundary |
| --- | --- | --- |
| Shared wire types | `dlssync-contracts` | Identity, evidence, plans, events, errors |
| Catalog | `dll-catalog` | Signed catalog, exact artifacts, source status |
| File inspection | `pe-version` | PE architecture, internal version, Authenticode |
| Application service | `dlssync-application` | Planning, execution, locks, durable recovery |
| GUI | Tauri commands | Adapt typed requests and emit service events |
| CLI and background | CLI adapters | Use the same application service |
| Presentation | Svelte domain stores | Consume snapshots; never infer successful installation |
| Distribution | Build configuration | Separate Standard and Nexus outputs |

## Wire contract

`cargo xtask generate-bindings` reflects the actual Tauri command arguments and
results. It also exports progress events and shared runtime types. Do not edit
`frontend/src/generated/bindings.ts` manually.

`cargo xtask check-bindings` generates to a temporary file and compares the bytes.
It does not rewrite the file being checked or close the running development app.

`invokeCommand` selects its return type from its command name. Callers cannot
provide an arbitrary return type. Required settings paths come from Rust's
serialized settings schema. An incomplete settings response is an error; the
frontend must not save defaults over the user's settings.

Legacy progress stages remain available during migration. New operations use
`OperationStage`, a monotonically increasing sequence, and an authoritative
`OperationSnapshot`. The receiver rejects duplicate and older sequences.
Subscriptions must exist before execution begins. Reconnection reads a snapshot.

New byte counts use decimal strings to preserve the full unsigned 64-bit range.
Legacy commands retain their existing numeric wire format. Version comparison
uses dotted version components, never a packed integer converted to JavaScript's
`number` type.

## Evidence and planning

`ComponentIdentity` identifies a game, relative path, filename, family, and PE
architecture. `ArtifactDescriptor` distinguishes internal file version from
package version. Expected publisher and observed signature are separate fields.
MD5 records retain their algorithm label; they are never described as SHA-256.

`ComponentState` contains the observation time, hash, candidate, compatibility
decision, and state revision. Unknown and experimental compatibility are explicit
states. An architecture or hash check does not prove that a game runs correctly.

`UpdatePlan` carries a schema version, catalog revision, file preconditions, exact
artifacts, and coherent-set dependencies. A persisted plan with schema version
zero predates the verified planner and must be rebuilt before execution. The
executor checks the plan fingerprint and re-reads files before mutation.

`DriverInstallPlan` binds a package to a device instance, hardware identities,
channel, installed version, recovery evidence, and an observation fingerprint.
An installer's exit code alone does not establish the installed driver version.

## Integration rule

Change the Rust owner first. Regenerate bindings, update consumers, and run the
contract and affected domain checks together. Keep adapters free of duplicate
compatibility decisions. Report `rolled_back` only after verifying restored
bytes; preserve `rollback_failed` and its recovery evidence.

This document defines the target boundary. Migration progress and acceptance
evidence belong to the local implementation plan, not to product claims.
