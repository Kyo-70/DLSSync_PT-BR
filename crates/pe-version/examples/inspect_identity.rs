fn main() {
    let path = std::env::args_os().nth(1).expect("DLL path");
    let path = std::path::Path::new(&path);
    println!(
        "{}",
        serde_json::json!({
            "identity": format!("{:?}", pe_version::read_pe_identity(path)),
            "signature": pe_version::read_authenticode(path),
        })
    );
}
