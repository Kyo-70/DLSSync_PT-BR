# Native platform boundaries

DLSSync's native build matrix covers Windows x64, Linux x64 on Ubuntu 24.04, and macOS 15 or later on Intel and Apple Silicon. The release asset list and successful checks establish which packages were actually produced; this guide is not a claim that an unpublished package exists.

## Capabilities

| Operation | Windows | Linux and macOS |
|---|---|---|
| Steam library discovery and local artwork | Native registry discovery | Native Steam directories, including Linux Flatpak |
| User-added game folders and PE/DLL inspection | Available | Available for Windows-game files in compatibility installations |
| Catalog browsing, downloads, journal and DLL backups | Available | Available |
| Publisher verification before DLL updates | Windows trust API | Bundled osslsigncode 2.14, primary signature only, native CA store |
| GPU driver installers, Windows Update devices, NVIDIA DRS profiles | Windows workflows | Not available; the driver UI explains the boundary |
| Automatic hardware recommendations | Uses observed adapters | No recommendation when adapters or required driver versions are unknown |

Native Linux/macOS games do not use Windows DLLs merely because DLSSync can discover their Steam folders. DLL replacement does not prove that a game works under Proton, Wine or CrossOver. A missing publisher, failed certificate chain, unavailable verifier or unmet driver requirement blocks an update. Native trust stores can differ from Windows trust stores; no verification bypass is recommended.

## Verification helper and source

`scripts/build-native-verifier.mjs` downloads osslsigncode source at commit `beec94e308d1a1e03ca17b05fe089d93c6303e90`, verifies its archive SHA-256 and builds the separate `dlssync-authenticode` executable with OpenSSL 3. The source archive, GPL terms, build instructions and OpenSSL license are included as package resources. The application invokes the bundled executable by absolute sibling path, verifies only signature index 0, and binds publisher identity to that signature's signer. Errors and timeouts remain failures.

Upstream references: [osslsigncode](https://github.com/mtrojnar/osslsigncode/tree/beec94e308d1a1e03ca17b05fe089d93c6303e90), [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Packaging and signatures

Linux packages require the supported GTK/WebKit environment. macOS packages built without Apple Developer credentials are ad-hoc signed and are not notarized. These facts are separate from the Tauri updater signature and from signatures on game DLLs. Published notes must state the actual signing result; do not describe ad-hoc signing as an identified Apple publisher.

Windows Standard and Nexus distribution rules remain unchanged. Native release inputs must use the same immutable source revision and must not enter the Windows Standard updater feed by filename guessing.
