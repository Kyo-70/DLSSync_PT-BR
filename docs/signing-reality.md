# Verify a download before responding to Windows warnings

Check the source and exact downloaded file before running DLSSync. Windows code signing, Tauri app-update signatures and vendor DLL signatures are different checks. None is a guarantee of harmless behavior, game compatibility or the absence of a Windows warning.

## What the repository establishes

The [release workflow](../.github/workflows/release.yml) has two explicit Windows signing lanes, selected by the repository variable `SIGNPATH_ENABLED`. With `true`, it signs the exact artifact set through SignPath and refuses to continue unless every file reports a valid Authenticode signature. Otherwise it publishes the same files unsigned, refuses any file that unexpectedly carries a signature, and records the real per-file state in `AUTHENTICODE-STATE.json`. Publication and the release notes both require a state that one of those lanes actually read, so a release cannot describe a signature that was never verified. A workflow that can sign is still not proof that the file you downloaded is signed: check the file itself.

An unsigned release is labeled as unsigned. Windows shows an unknown-publisher warning for it, that warning is correct, and no DLSSync release asks you to turn off SmartScreen, Windows Defender or any other protection to install it.

The Standard app updater signs its payload with an Ed25519 key in both lanes, and publication fails without a nonempty matching signature in `latest.json`. A Tauri update signature is not the installer's Windows Authenticode signature. Likewise, an NVIDIA/AMD/Intel/Microsoft signature on a downloaded game DLL does not sign the DLSSync application.

DLSSync's own signature state does not change how it treats game DLLs. The apply path still requires a trusted Authenticode publisher for a downloaded DLL before it replaces a file. Check the published release notes and the file itself for the signature state of the version you downloaded. Claims about guaranteed warning removal, reputation thresholds or prompts occurring once per version are not supported here.

## Check the intended artifact

1. Start from [GitHub Releases](https://github.com/xt0n1-t3ch/DLSSync/releases/latest) for Standard or [Nexus Mods, mod 1922](https://www.nexusmods.com/site/mods/1922) for Nexus. Check the version, channel and Windows x64 format.
2. Inspect the exact file's Windows signature, not a screenshot or another asset's signature. Check the publisher and signature status if present.
3. Compare a published digest when one is available from a trusted release source. Matching a digest establishes byte equality with that reference, not universal safety or compatibility.
4. If the source, signature or warning is unexpected, stop and ask the maintainer. Do not disable antivirus/SmartScreen or use a repack to bypass it.

You can inspect a downloaded installer in PowerShell, replacing the placeholder with the actual relative filename:

```powershell
Get-AuthenticodeSignature -LiteralPath '.\downloaded_installer.exe' |
  Format-List Status, StatusMessage, SignerCertificate
Get-FileHash -LiteralPath '.\downloaded_installer.exe' -Algorithm SHA256
```

A missing signature, invalid signature and valid signature are different results. This guide does not advise clicking through a Windows warning solely because the app is open source or because a previous release was trusted.

## Choose the format separately from trust

NSIS is the configured current-user installer and normal Standard updater route. MSI is a Windows Installer deployment format. Portable ZIP avoids an app installation step but still contains executable code; extraction does not establish trust. Portable mode disables in-app app updates and uses executable-relative data when its marker is present.

See [installation](../README.md#download) and [Nexus channel rules](nexus-build.md). Neither a package format nor optional signing configuration guarantees that Windows will omit a reputation or security prompt.

## Maintainer boundary

Verify signatures and source identity on the actual artifacts before publishing claims about them. The [SignPath configuration notes](../.github/workflows/SIGNPATH.md) describe setup, not proof that a release was signed. No external signing-program claims, reputation metrics or warning-removal promises are relied on here.

Maintainers verify the Standard updater signature against the public key shipped in the app before publication:

```powershell
cargo xtask verify-updater-signature --installer release/DLSSync_1.7.0_x64-setup.exe
```

The verifier reads the adjacent `.sig` file and the configured public key. Missing, malformed, mismatched or tampered inputs fail verification.
