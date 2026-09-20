# Identify a game's DLSS, FSR and XeSS DLLs

Use this map to understand the components shown for a scanned game. DLSSync updates recognized libraries already integrated into a game; copying a DLL does not add an absent rendering feature or make unsupported hardware compatible.

## Match the file to its role

Super Resolution (SR) is the upscaling component; Frame Generation (FG) produces additional frames; Ray Reconstruction (RR) is a separate reconstruction component. These roles have distinct files and catalog identities.

| Family | Recognized filenames | What to keep separate |
|---|---|---|
| NVIDIA DLSS SR | `nvngx_dlss.dll` | The SR runtime is not the Streamline plugin. |
| NVIDIA DLSS FG | `nvngx_dlssg.dll` | FG has its own runtime and hardware/driver constraints. |
| NVIDIA DLSS RR | `nvngx_dlssd.dll` | RR is not an SR preset or an FG file. |
| NVIDIA Streamline DLSS plugins | `sl.dlss.dll`, `sl.dlss_g.dll`, `sl.dlss_d.dll` | These are Streamline package members, not aliases for the `nvngx_` runtimes. |
| Streamline infrastructure and plugins | `sl.interposer.dll`, `sl.common.dll`, `sl.pcl.dll`, `sl.nis.dll`, `sl.directsr.dll`, `sl.reflex.dll` | Use a coherent Streamline set, including Reflex where selected. |
| Intel XeSS SR | `libxess.dll`, `libxess_dx11.dll` | DirectX variants are not interchangeable filenames. |
| Intel XeSS FG and XeLL low-latency runtime | `libxess_fg.dll`, `libxell.dll` | Catalog dependencies associate XeSS FG with XeLL. |
| AMD FSR upscaler | `amd_fidelityfx_dx12.dll`, `amd_fidelityfx_upscaler_dx12.dll`, `ffx_fsr3upscaler_x64.dll` | Legacy and current names need the matching catalog artifact. |
| AMD FSR Vulkan upscaler | `amd_fidelityfx_vk.dll` | Vulkan is separate from the DirectX 12 artifact. |
| AMD FSR FG | `amd_fidelityfx_framegeneration_dx12.dll`, `ffx_frameinterpolation_x64.dll` | Do not substitute an upscaler for frame interpolation. |
| AMD FSR loader | `amd_fidelityfx_loader_dx12.dll` | Loader and runtime versions can differ. |
| AMD FSR denoiser | `amd_fidelityfx_denoiser_dx12.dll` | This has a distinct catalog family. |
| Microsoft DirectStorage | `dstorage.dll`, `dstoragecore.dll` | Keep both from the same package. |

This is a scanner filename map, not a supported-game or hardware test matrix. A listed filename does not guarantee a current download candidate. Catalog availability, architecture, feature settings, package dependencies and minimum-driver checks can change what the app offers.

## Choose an update

1. Close the game and scan its install folder in **Library**.
2. Open its details and inspect each component's filename, installed version and candidate.
3. Leave related package members together. Use the game's update action rather than copying a similarly named file from another API or package.
4. Read the result and test the game. Use [Backups](restoring-backups.md) if the replacement causes a regression.

A game with no recognized DLLs may use a different integration, a statically linked implementation or no supported feature. The scanner result alone does not establish which explanation applies. Adding unsupported integrations is not documented as a DLSSync workflow.

## Evidence and related guides

Source review: 2026-09-13. Filename/family mapping comes from [`KNOWN_DLLS` and `DllFamily`](../crates/dll-scanner/src/lib.rs); dependency and artifact identity rules come from [catalog v3](../crates/dll-catalog/src/v3.rs) and [catalog format](catalog-format.md). These are source observations, not game launch tests.

Read [Streamline sets](streamline.md), [versions and compatibility](versions-and-compatibility.md), and [optional mods](optional-mods.md) before changing a mixed or mod-managed installation.
