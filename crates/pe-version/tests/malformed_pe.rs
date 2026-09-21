#[test]
fn malformed_optional_header_returns_error_without_aborting_process() {
    let mut bytes = vec![0; 4096];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    assert!(pe_version::parse_bytes(&bytes).is_err());
}
