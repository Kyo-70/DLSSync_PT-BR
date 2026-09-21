/// Export command metadata without compiling or starting the main app binary.
fn main() {
    dlssync_lib::ipc_bindings::export_typescript_bindings();
}
