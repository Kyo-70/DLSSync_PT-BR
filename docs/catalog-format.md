# Catalog formats and source verification

DLSSync keeps two independently signed documents:

- `manifest.json` preserves schema v2 for existing clients.
- `manifest-v3.json` adds exact artifact identity, source health, and dependencies.

The generator derives v2 from the same accepted observations as v3. It removes
every v3 field before writing v2. It signs the final UTF-8 bytes of each document
with Ed25519. JSON formatting changes invalidate that signature.

## Artifact identity

Each inspected artifact records its filename, internal PE version, package
version, package identity, architecture, SHA-256 digest, exact source URL, and
archive entry. The package identity groups files whose internal version numbers
can differ. A loader version must not describe its upscaler or frame generator.

Expected publisher and observed publisher are separate. Signature status is
`not_checked`, `missing`, `verified`, or `untrusted`. A repository owner or vendor
name does not establish the signer of a file. The Windows generator inspects
Authenticode; other platforms report the absence of that observation.

Historical community records can retain MD5. The algorithm is explicit. New
community artifacts and current candidates receive SHA-256 after inspection.
Consumers must reject a digest whose length disagrees with its algorithm.

The generator rejects foreign PE architecture, ambiguous archive identity,
traversal paths, and oversized data. The application must repeat architecture,
integrity, and publisher checks before replacing a game file. A signed catalog
does not establish game compatibility.

## Upstream sources

| Source | Retrieval and evidence |
| --- | --- |
| Streamline | Official x64 release archive; mandatory common library and interposer |
| XeSS | Official SDK archive; XeSS-FG requires XeLL |
| FidelityFX | Signed runtime directories at immutable Git commits, including legacy filenames |
| DirectStorage | Official NuGet packages; both DLLs from the same package |
| Community history | Existing explicit hashes; new candidates inspected before publication |

AMD's recent release archives contain samples. The generator reads the signed
runtime directories from the SDK repository instead of assuming those archives
are SDK packages. Package tags remain separate from PE file versions.

The dependency rules follow the [Streamline distribution guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuide.md#4-distributing-sl-with-your-application)
and [Intel XeSS SDK](https://github.com/intel/xess). Other installed members of a
package belong to the planner's coherent update set; they are not all mandatory
dependencies of every feature.

## Freshness and failure

Every source records its last attempt, last successful query, affected families,
and current error. Failed ingestion keeps the previous data and successful-query
time. A new global generation time must not conceal a stale source.

Archive caching uses ETag requests and verifies cached bytes against their saved
SHA-256 before reuse. Download size limits apply even without Content-Length.
GitHub authentication is attached only to GitHub API requests. It is never a
default header on a client shared with third-party downloads.

## Generate and validate

Set `DLSSYNC_MANIFEST_SIGNING_KEY` through the local secret store or CI secret.
Never commit or print its value. Set `GITHUB_TOKEN` for GitHub API capacity.

```powershell
cargo run -p manifest-builder -- --out manifest/manifest.json --out-v3 manifest/manifest-v3.json
```

Existing output files and detached signatures seed the previous valid catalog.
`--sources` selects providers for a targeted refresh. `--dry-run` reports the
selected sources and output paths without fetching or publishing anything.

The manifest repository validates schemas, signatures, architecture hints,
artifact identities, dependencies, and the semantic change before publication.
Runtime file validation remains mandatory after publication.

The publisher runs on Windows and reads the exact generator commit from
`builder-ref.txt` in the manifest repository. Tests, schema checks, identity
checks, and both detached signatures must pass before it commits new bytes.
The Standard client uses v3. Nexus retains its pinned v2 fallback and uses v3
only when the user explicitly refreshes the catalog.
