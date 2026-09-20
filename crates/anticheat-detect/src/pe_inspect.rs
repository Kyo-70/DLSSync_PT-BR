use crate::entropy::shannon;
use crate::signatures::{
    HitSource, ProtectionKind, DENUVO_SECTION_CLUSTER, DENUVO_SECTION_CLUSTER_MIN,
    PACKED_ENTROPY_THRESHOLD, PROTECTOR_SECTIONS, PROTECTOR_SECTION_COUNT_HINT, STRING_MARKERS,
};
use crate::ProtectionHit;
use std::collections::BTreeSet;
use std::path::Path;

/// Entropy is sampled over at most this many bytes per section. Denuvo binaries
/// can be hundreds of MB; a 64 KiB sample is representative of packed-ness and
/// keeps the scan fast.
const ENTROPY_SAMPLE_BYTES: usize = 64 * 1024;

/// Inspect a single PE file (game executable or DLL) for protector fingerprints.
/// Memory-maps the file so large Denuvo binaries are not read into the heap.
pub fn inspect_pe(path: &Path) -> Vec<ProtectionHit> {
    match pelite::FileMap::open(path) {
        Ok(map) => inspect_bytes(map.as_ref()),
        Err(_) => Vec::new(),
    }
}

/// Parse section fields through bounded byte slices. PE optional-header lengths
/// are untrusted; they must never become aligned Rust references.
fn section_samples(bytes: &[u8]) -> Option<Vec<(String, f64)>> {
    let word = |offset: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let dword = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = dword(60)? as usize;
    if !(64..=16 * 1024 * 1024).contains(&pe) || bytes.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let count = word(pe + 6)? as usize;
    let optional_size = word(pe + 20)? as usize;
    let minimum = match word(pe + 24)? {
        0x10b => 96,
        0x20b => 112,
        _ => return None,
    };
    if count > 96 || optional_size < minimum || !optional_size.is_multiple_of(4) {
        return None;
    }
    let sections = pe.checked_add(24)?.checked_add(optional_size)?;
    let table = bytes.get(sections..sections.checked_add(count.checked_mul(40)?)?)?;
    let mut result = Vec::with_capacity(count);
    for section in table.chunks_exact(40) {
        let name_end = section[..8].iter().position(|byte| *byte == 0).unwrap_or(8);
        let name = std::str::from_utf8(&section[..name_end])
            .unwrap_or_default()
            .to_string();
        let size = u32::from_le_bytes(section[16..20].try_into().ok()?) as usize;
        let start = u32::from_le_bytes(section[20..24].try_into().ok()?) as usize;
        let sample = start
            .checked_add(size)
            .and_then(|end| bytes.get(start..end));
        let entropy = sample
            .map(|data| shannon(&data[..data.len().min(ENTROPY_SAMPLE_BYTES)]))
            .unwrap_or(0.0);
        result.push((name, entropy));
    }
    Some(result)
}

pub fn inspect_bytes(bytes: &[u8]) -> Vec<ProtectionHit> {
    let mut hits: Vec<ProtectionHit> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for (needle, name, kind) in STRING_MARKERS {
        if memchr::memmem::find(bytes, needle).is_some() {
            push(&mut hits, &mut seen, name, *kind);
        }
    }

    let Some(sections) = section_samples(bytes) else {
        return hits;
    };

    let mut max_entropy = 0.0f64;
    let mut packed_count = 0usize;
    let mut denuvo_cluster = 0usize;
    for (name, entropy) in &sections {
        if *entropy > max_entropy {
            max_entropy = *entropy;
        }
        if *entropy >= PACKED_ENTROPY_THRESHOLD {
            packed_count += 1;
        }
        let low = name.to_ascii_lowercase();
        for (sec, proto, kind) in PROTECTOR_SECTIONS {
            if low == *sec || low.starts_with(&format!("{sec}\0")) {
                push(&mut hits, &mut seen, proto, *kind);
            }
        }
        if DENUVO_SECTION_CLUSTER.iter().any(|c| low == *c) {
            denuvo_cluster += 1;
        }
    }

    if denuvo_cluster >= DENUVO_SECTION_CLUSTER_MIN {
        push(
            &mut hits,
            &mut seen,
            "Denuvo Anti-Tamper",
            ProtectionKind::AntiTamper,
        );
    }

    let named_anti_tamper = hits.iter().any(|h| h.kind == ProtectionKind::AntiTamper);
    if !named_anti_tamper
        && sections.len() >= PROTECTOR_SECTION_COUNT_HINT
        && packed_count >= PROTECTOR_SECTION_COUNT_HINT / 2
        && max_entropy >= PACKED_ENTROPY_THRESHOLD
    {
        push(
            &mut hits,
            &mut seen,
            "Unknown anti-tamper (heuristic)",
            ProtectionKind::AntiTamper,
        );
    }

    hits
}

fn push(
    hits: &mut Vec<ProtectionHit>,
    seen: &mut BTreeSet<String>,
    name: &str,
    kind: ProtectionKind,
) {
    if seen.insert(name.to_string()) {
        hits.push(ProtectionHit {
            name: name.to_string(),
            kind,
            source: HitSource::Pe,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_optional_header_does_not_abort_section_scan() {
        let mut bytes = build_pe64(&[]);
        bytes[84..86].copy_from_slice(&2u16.to_le_bytes());
        assert!(inspect_bytes(&bytes).is_empty());
    }

    fn build_pe64(sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let num = sections.len() as u16;
        let opt_size: u16 = 240;
        let headers_size = 0x40 + 4 + 20 + opt_size as usize + sections.len() * 40;
        let file_align = 0x200usize;
        let headers_padded = headers_size.div_ceil(file_align) * file_align;

        let mut buf = vec![0u8; headers_padded];
        buf[0] = b'M';
        buf[1] = b'Z';
        let e_lfanew = 0x40u32;
        buf[0x3c..0x40].copy_from_slice(&e_lfanew.to_le_bytes());
        let mut o = e_lfanew as usize;
        buf[o..o + 4].copy_from_slice(b"PE\0\0");
        o += 4;
        buf[o..o + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        buf[o + 2..o + 4].copy_from_slice(&num.to_le_bytes());
        buf[o + 16..o + 18].copy_from_slice(&opt_size.to_le_bytes());
        buf[o + 18..o + 20].copy_from_slice(&0x22u16.to_le_bytes());
        o += 20;
        let opt_start = o;
        buf[o..o + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        buf[o + 16..o + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[opt_start + 24..opt_start + 32].copy_from_slice(&0x140000000u64.to_le_bytes());
        buf[opt_start + 32..opt_start + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[opt_start + 36..opt_start + 40].copy_from_slice(&(file_align as u32).to_le_bytes());
        buf[opt_start + 56..opt_start + 60].copy_from_slice(&0x10000u32.to_le_bytes());
        buf[opt_start + 60..opt_start + 64].copy_from_slice(&(headers_padded as u32).to_le_bytes());
        buf[opt_start + 108..opt_start + 112].copy_from_slice(&16u32.to_le_bytes());
        o = opt_start + opt_size as usize;

        let mut raw_ptr = headers_padded;
        let mut va = 0x1000u32;
        let mut bodies: Vec<(usize, Vec<u8>)> = Vec::new();
        for (name, data) in sections {
            let mut nm = [0u8; 8];
            let nb = name.as_bytes();
            nm[..nb.len().min(8)].copy_from_slice(&nb[..nb.len().min(8)]);
            buf[o..o + 8].copy_from_slice(&nm);
            let raw_size = data.len().div_ceil(file_align) * file_align;
            buf[o + 8..o + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
            buf[o + 12..o + 16].copy_from_slice(&va.to_le_bytes());
            buf[o + 16..o + 20].copy_from_slice(&(raw_size as u32).to_le_bytes());
            buf[o + 20..o + 24].copy_from_slice(&(raw_ptr as u32).to_le_bytes());
            buf[o + 36..o + 40].copy_from_slice(&0x60000020u32.to_le_bytes());
            bodies.push((raw_ptr, data.clone()));
            raw_ptr += raw_size;
            va += (data.len().div_ceil(0x1000) * 0x1000) as u32;
            o += 40;
        }
        buf.resize(raw_ptr, 0);
        for (ptr, data) in bodies {
            buf[ptr..ptr + data.len()].copy_from_slice(&data);
        }
        buf
    }

    fn high_entropy(len: usize) -> Vec<u8> {
        let mut s = 0x1234_5678u32;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (s >> 16) as u8
            })
            .collect()
    }

    #[test]
    fn detects_vmprotect_by_section_name() {
        let pe = build_pe64(&[
            (".text", vec![0x90; 0x400]),
            (".vmp0", high_entropy(0x2000)),
        ]);
        let hits = inspect_bytes(&pe);
        assert!(hits
            .iter()
            .any(|h| h.name == "VMProtect" && h.kind == ProtectionKind::AntiTamper));
    }

    #[test]
    fn detects_steam_ceg_drm_section_as_drm() {
        let pe = build_pe64(&[(".text", vec![0x90; 0x400]), (".bind", vec![0x11; 0x800])]);
        let hits = inspect_bytes(&pe);
        assert!(hits
            .iter()
            .any(|h| h.name == "Steam CEG DRM" && h.kind == ProtectionKind::Drm));
    }

    #[test]
    fn detects_denuvo_string_marker_anywhere() {
        let mut body = vec![0x90u8; 0x400];
        body.extend_from_slice(b"....denuvo_atd....");
        let pe = build_pe64(&[(".text", body)]);
        let hits = inspect_bytes(&pe);
        assert!(hits
            .iter()
            .any(|h| h.name == "Denuvo Anti-Tamper" && h.kind == ProtectionKind::AntiTamper));
    }

    #[test]
    fn clean_pe_yields_no_detection() {
        let pe = build_pe64(&[(".text", vec![0x90; 0x400]), (".rdata", vec![0x00; 0x400])]);
        let hits = inspect_bytes(&pe);
        assert!(hits.is_empty(), "expected clean, got {hits:?}");
    }

    #[test]
    fn detects_denuvo_by_section_name_cluster() {
        let mut secs: Vec<(&str, Vec<u8>)> = vec![(".text", vec![0x90; 0x400])];
        for n in [".arch", ".shared", ".data1", ".data2", ".sxdata", ".xtext"] {
            secs.push((n, vec![0x42; 0x400]));
        }
        let pe = build_pe64(&secs);
        let hits = inspect_bytes(&pe);
        assert!(
            hits.iter()
                .any(|h| h.name == "Denuvo Anti-Tamper" && h.kind == ProtectionKind::AntiTamper),
            "Denuvo section cluster should be detected, got {hits:?}"
        );
    }

    #[test]
    fn generic_heuristic_fires_on_many_packed_non_cluster_sections() {
        let mut secs: Vec<(&str, Vec<u8>)> = vec![(".text", vec![0x90; 0x400])];
        let names = [
            ".pk0", ".pk1", ".pk2", ".pk3", ".pk4", ".pk5", ".pk6", ".pk7", ".pk8", ".pk9", ".pka",
            ".pkb", ".pkc", ".pkd",
        ];
        for n in names {
            secs.push((n, high_entropy(0x2000)));
        }
        let pe = build_pe64(&secs);
        let hits = inspect_bytes(&pe);
        assert!(hits
            .iter()
            .any(|h| h.name == "Unknown anti-tamper (heuristic)"));
    }

    #[test]
    fn non_pe_bytes_yield_no_detection_without_panic() {
        assert!(inspect_bytes(b"not a pe at all").is_empty());
        assert!(inspect_bytes(&[]).is_empty());
    }
}
