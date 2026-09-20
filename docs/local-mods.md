# Install and remove local mods

DLSSync can apply a schema-1 local mod recipe to a scanned game. A recipe describes exact files, hashes and game executable bytes. The mod catalog currently contains no reviewed mods. A local import does not prove compatibility or official support.

## Install files

1. Scan the library and open the game details.
2. Expand **Mods**. Choose the recipe JSON file.
3. Choose the folder containing the extracted files you downloaded.
4. Select **Install files**, then **Check files**.
5. Read any conflicts or rejected checks. Select **Install mod** when the check allows it.

A check expires after five minutes and works once. The backend checks the game executable again before writing. No network request downloads a mod. Existing files can only be replaced when a committed receipt proves ownership and their current hashes still match.

## Change settings or remove a mod

Select **Change settings** with the same recipe to apply supported INI key edits. The backend preserves unrelated bytes. On removal, it restores a key only while its value still matches the value the mod wrote.

Use **Remove mod** beside an installed entry. DLSSync verifies the restoration. If external changes prevent removal, it retains those bytes and reports the failure. Recovery records persist across application restarts and are checked when the installation is reopened.

## Supported boundaries

- The target game ID and executable SHA-256 must match the recipe.
- Local protection detection and a running game block changes.
- Catalog-owned DLL filenames cannot be claimed by a mod.
- The local adapter accepts extracted files. It does not download or extract archives.
- Recipes with dependencies, required capability checks or declared conflicts that this adapter cannot evaluate are rejected.
- Configuration edits use lossless INI semantics with case-insensitive keys. Multiple edits to the same file are currently rejected as duplicate transaction targets.
- Reported game-test evidence inside a local file does not become a verified compatibility claim.

Local tests do not establish compatibility with a running game or with hardware that was not tested here.
