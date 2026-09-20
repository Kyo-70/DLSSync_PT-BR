//! Bounded PE identity inspection, independent of version resources and signing.
use dlssync_contracts::Architecture;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const MAX_HEADER_OFFSET: u64 = 16 * 1024 * 1024;
const COFF_BYTES: usize = 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeIdentity {
    pub architecture: Architecture,
    pub is_dll: bool,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn pe_offset(dos: &[u8]) -> io::Result<u64> {
    if dos.len() < 64 || &dos[..2] != b"MZ" {
        return Err(invalid("missing DOS header"));
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    if !(64..=MAX_HEADER_OFFSET).contains(&offset) {
        return Err(invalid("PE header offset out of bounds"));
    }
    Ok(offset)
}

fn parse_coff(header: &[u8]) -> io::Result<PeIdentity> {
    if header.len() < COFF_BYTES || &header[..4] != b"PE\0\0" {
        return Err(invalid("missing PE signature or truncated COFF header"));
    }
    let word = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]);
    let architecture = match word(4) {
        0x014c => Architecture::X86,
        0x8664 => Architecture::X64,
        0xaa64 => Architecture::Arm64,
        0xa641 => Architecture::Arm64Ec,
        _ => Architecture::Unknown,
    };
    if word(20) < 2 {
        return Err(invalid("missing PE optional header"));
    }
    let expected_magic = if architecture == Architecture::X86 {
        0x10b
    } else {
        0x20b
    };
    if word(24) != expected_magic {
        return Err(invalid("PE machine and optional header disagree"));
    }
    Ok(PeIdentity {
        architecture,
        is_dll: word(22) & 0x2000 != 0,
    })
}

pub fn inspect_pe(bytes: &[u8]) -> io::Result<PeIdentity> {
    let offset = pe_offset(bytes)? as usize;
    let end = offset
        .checked_add(COFF_BYTES)
        .ok_or_else(|| invalid("PE offset overflow"))?;
    parse_coff(
        bytes
            .get(offset..end)
            .ok_or_else(|| invalid("truncated PE header"))?,
    )
}

pub fn read_pe_identity(path: &Path) -> io::Result<PeIdentity> {
    let mut file = std::fs::File::open(path)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    let offset = pe_offset(&dos)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0; COFF_BYTES];
    file.read_exact(&mut header)?;
    parse_coff(&header)
}

/// This release only replaces x64 DLLs. A valid signature never bypasses PE checks.
pub fn require_x64_dll_pair(installed: &Path, candidate: &Path) -> io::Result<()> {
    for path in [installed, candidate] {
        let identity = read_pe_identity(path)?;
        if identity.architecture != Architecture::X64 || !identity.is_dll {
            return Err(invalid(&format!(
                "incompatible PE identity for {}: {:?}, DLL={}; x64 DLL required",
                path.display(),
                identity.architecture,
                identity.is_dll
            )));
        }
    }
    Ok(())
}

pub fn require_x64_executable(path: &Path) -> io::Result<()> {
    let identity = read_pe_identity(path)?;
    if identity.architecture != Architecture::X64 || identity.is_dll {
        return Err(invalid(&format!(
            "{} is not an x64 executable",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(machine: u16, dll: bool) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        bytes[84..86].copy_from_slice(&240u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&(if dll { 0x2000u16 } else { 0 }).to_le_bytes());
        bytes[88..90]
            .copy_from_slice(&(if machine == 0x14c { 0x10bu16 } else { 0x20b }).to_le_bytes());
        bytes
    }
    #[test]
    fn distinguishes_all_upstream_architectures_without_filename_guesses() {
        for (machine, expected) in [
            (0xaa64, Architecture::Arm64),
            (0xa641, Architecture::Arm64Ec),
            (0x8664, Architecture::X64),
            (0x14c, Architecture::X86),
        ] {
            assert_eq!(
                inspect_pe(&header(machine, true)).unwrap(),
                PeIdentity {
                    architecture: expected,
                    is_dll: true
                }
            );
        }
        assert!(!inspect_pe(&header(0x8664, false)).unwrap().is_dll);
    }
    #[test]
    fn rejects_truncation_and_hostile_offsets() {
        let bytes = header(0x8664, true);
        for len in 0..90 {
            assert!(inspect_pe(&bytes[..len]).is_err());
        }
        let mut hostile = bytes;
        hostile[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(inspect_pe(&hostile).is_err());
    }
    #[test]
    fn rejects_mismatched_optional_header() {
        let mut bytes = header(0x8664, true);
        bytes[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
        assert!(inspect_pe(&bytes).is_err());
    }

    #[test]
    fn executable_validation_rejects_arm_x86_and_disguised_dlls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.exe");
        for (machine, dll) in [(0xaa64, false), (0x14c, false), (0x8664, true)] {
            std::fs::write(&path, header(machine, dll)).unwrap();
            assert!(require_x64_executable(&path).is_err());
        }
        std::fs::write(&path, header(0x8664, false)).unwrap();
        assert!(require_x64_executable(&path).is_ok());
    }

    #[test]
    fn wrong_candidate_is_rejected_without_changing_installed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("installed.dll");
        let candidate = dir.path().join("candidate.dll");
        let original = header(0x8664, true);
        std::fs::write(&installed, &original).unwrap();
        for machine in [0xaa64, 0xa641, 0x14c] {
            std::fs::write(&candidate, header(machine, true)).unwrap();
            assert!(require_x64_dll_pair(&installed, &candidate).is_err());
            assert_eq!(std::fs::read(&installed).unwrap(), original);
        }
        std::fs::write(&candidate, header(0x8664, false)).unwrap();
        assert!(require_x64_dll_pair(&installed, &candidate).is_err());
        std::fs::write(&candidate, &original).unwrap();
        assert!(require_x64_dll_pair(&installed, &candidate).is_ok());
    }
}
