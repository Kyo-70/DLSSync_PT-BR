fn main() {
    tauri_build::build();
    #[cfg(target_os = "windows")]
    embed_resource::compile_for_examples("tests/commands.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
