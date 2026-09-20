use super::AuthenticodeInfo;

// Only the verified primary signature's signer is used. Timestamp certificates
// and nested signatures must not supply a different publisher identity.
fn parse_result(output: &str, success: bool) -> AuthenticodeInfo {
    let mut primary = false;
    let mut signer = false;
    let mut subject = None;
    let mut issuer = None;
    for line in output.lines().map(str::trim) {
        if line.starts_with("Signature Index:") {
            primary = line.starts_with("Signature Index: 0 ");
            signer = false;
        } else if primary && line == "Signer #0:" && subject.is_none() {
            signer = true;
        } else if signer {
            if let Some(value) = line.strip_prefix("Subject: ") {
                subject = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("Issuer : ") {
                issuer = Some(value.to_owned());
                break;
            }
        }
    }
    let cn = subject.as_deref().and_then(common_name);
    AuthenticodeInfo {
        trusted: success && cn.is_some(),
        subject_cn: cn,
        subject_dn: subject,
        issuer_dn: issuer,
        status: if success {
            "osslsigncode verification succeeded"
        } else {
            "osslsigncode verification failed"
        }
        .into(),
        revocation_bypassed: false,
    }
}

// Decode the limited RFC2253 spelling used by the vendor allowlist. Ambiguous
// multi-valued names, duplicate CNs and hex escapes fail closed.
fn common_name(dn: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut escaped = false;
    for character in dn.chars() {
        if escaped {
            if !matches!(
                character,
                ',' | '+' | '"' | '\\' | '<' | '>' | ';' | ' ' | '#'
            ) {
                return None;
            }
            part.push(character);
            escaped = false;
        } else {
            match character {
                '\\' => escaped = true,
                ',' => parts.push(std::mem::take(&mut part)),
                '+' => return None,
                _ => part.push(character),
            }
        }
    }
    if escaped {
        return None;
    }
    parts.push(part);
    let names: Vec<_> = parts
        .iter()
        .filter_map(|part| part.strip_prefix("CN="))
        .collect();
    (names.len() == 1 && !names[0].is_empty()).then(|| names[0].to_string())
}

#[cfg(not(windows))]
pub(super) fn read(path: &std::path::Path) -> AuthenticodeInfo {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let failure = |status: String| AuthenticodeInfo {
        trusted: false,
        subject_cn: None,
        subject_dn: None,
        issuer_dn: None,
        status,
        revocation_bypassed: false,
    };
    let executable = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("dlssync-authenticode")))
    {
        Some(path) if path.is_file() => path,
        _ => return failure("Bundled Authenticode verifier is unavailable".into()),
    };
    let mut child = match Command::new(executable)
        .args(["verify", "-index", "0", "-in"])
        .arg(path)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return failure(format!("Cannot start Authenticode verifier: {error}")),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < Duration::from_secs(30) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return failure("Authenticode verifier timed out or could not be observed".into());
            }
        }
    }
    match child.wait_with_output() {
        Ok(output) => parse_result(
            &String::from_utf8_lossy(&output.stdout),
            output.status.success(),
        ),
        Err(error) => failure(format!("Cannot read Authenticode verification: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_identity_to_the_primary_signer_and_process_result() {
        let output = "Subject: CN=Spoof\nSignature Index: 0  (Primary Signature)\nSigner #0:\nSubject: CN=NVIDIA Corporation,O=NVIDIA Corporation\nIssuer : CN=Issuer\nSignature Index: 1\nSigner #0:\nSubject: CN=Other\nIssuer : CN=Other";
        assert_eq!(
            parse_result(output, true).subject_cn.as_deref(),
            Some("NVIDIA Corporation")
        );
        assert!(parse_result(output, true).trusted);
        assert!(!parse_result(output, false).trusted);
        assert!(!parse_result("Subject: CN=NVIDIA Corporation", true).trusted);
    }

    #[test]
    fn decodes_vendor_comma_but_rejects_ambiguous_names() {
        assert_eq!(
            common_name(r"CN=Advanced Micro Devices\, Inc.,O=AMD"),
            Some("Advanced Micro Devices, Inc.".into())
        );
        assert_eq!(common_name("CN=NVIDIA Corporation,CN=Other"), None);
        assert_eq!(common_name("CN=NVIDIA Corporation+OU=Other"), None);
        assert_eq!(common_name(r"CN=\4eVIDIA Corporation"), None);
    }
}
