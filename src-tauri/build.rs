fn main() {
    #[cfg(not(feature = "nexus"))]
    tauri_build::build();
    #[cfg(feature = "nexus")]
    {
        // Tauri validates capability files before selecting inline config capabilities.
        // Derive an updater-free build input without changing the Standard owner on disk.
        println!("cargo:rerun-if-changed=capabilities/default.json");
        let mut capability: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string("capabilities/default.json").expect("read default capability"),
        )
        .expect("parse default capability");
        capability
            .as_object_mut()
            .expect("capability object")
            .remove("$schema");
        capability["permissions"]
            .as_array_mut()
            .expect("capability permissions")
            .retain(|permission| permission.as_str() != Some("updater:default"));
        let path =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("build output directory"))
                .join("nexus-default.capability.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&capability).expect("serialize Nexus capability"),
        )
        .expect("write Nexus capability");
        let pattern: &'static str =
            Box::leak(path.to_string_lossy().replace('\\', "/").into_boxed_str());
        tauri_build::try_build(tauri_build::Attributes::new().capabilities_path_pattern(pattern))
            .expect("build Nexus with its updater-free capability");
    }
    #[cfg(target_os = "windows")]
    {
        embed_resource::compile_for_examples("tests/commands.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
