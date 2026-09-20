declare const __DLSSYNC_TARGET_OS__: string;

// Tauri supplies the build target. Browser development keeps the Windows default.
// Native command handlers remain the authority for operation availability.
export const supportsWindowsDrivers = typeof __DLSSYNC_TARGET_OS__ === "undefined"
  || ["windows", "win32"].includes(__DLSSYNC_TARGET_OS__);
