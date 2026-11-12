//! Read-only D0 diagnostic probe for NixOS booted/current deployment identity.
//!
//! This tool intentionally does not return release-qualifying evidence. The
//! current userspace symlink can change without a reboot, so it is reported
//! separately from the system that actually booted. Local root-readable files
//! and paths are diagnostics, not authenticated device attestation.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SCHEMA: &str = "srq-boot-observation-diagnostic-v1";
const BOOTED_LINK: &str = "run/booted-system";
const CURRENT_LINK: &str = "run/current-system";
const BOOT_ID_FILE: &str = "proc/sys/kernel/random/boot_id";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Match,
    Mismatch,
    Blocked,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Match => "MATCH",
            Self::Mismatch => "MISMATCH",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiagnosticRecord {
    status: Status,
    reason_code: &'static str,
    boot_instance_id: Option<String>,
    booted_deployment_id: Option<String>,
    current_userspace_deployment_id: Option<String>,
}

impl DiagnosticRecord {
    fn evaluate(root: &Path) -> Self {
        let boot_instance_id = read_boot_id(root);
        let booted_deployment_id = resolve_store_link(root, Path::new(BOOTED_LINK)).ok();
        let current_userspace_deployment_id =
            resolve_store_link(root, Path::new(CURRENT_LINK)).ok();

        let missing_or_invalid_boot_id = boot_instance_id.is_none();
        let missing_or_invalid_booted = booted_deployment_id.is_none();
        let missing_or_invalid_current = current_userspace_deployment_id.is_none();

        if missing_or_invalid_boot_id || missing_or_invalid_booted || missing_or_invalid_current {
            let reason_code = if missing_or_invalid_boot_id {
                "boot-id-missing-or-invalid"
            } else if missing_or_invalid_booted {
                "booted-system-missing-or-invalid"
            } else {
                "current-system-missing-or-invalid"
            };
            return Self {
                status: Status::Blocked,
                reason_code,
                boot_instance_id,
                booted_deployment_id,
                current_userspace_deployment_id,
            };
        }

        let booted = booted_deployment_id.as_deref().unwrap_or_default();
        let current = current_userspace_deployment_id
            .as_deref()
            .unwrap_or_default();
        let (status, reason_code) = if booted == current {
            (Status::Match, "booted-and-current-identities-match")
        } else {
            (
                Status::Mismatch,
                "current-userspace-differs-from-booted-system",
            )
        };

        Self {
            status,
            reason_code,
            boot_instance_id,
            booted_deployment_id,
            current_userspace_deployment_id,
        }
    }

    fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"schema\":\"{}\",",
                "\"evidence_tier\":\"D0_LOCAL_DIAGNOSTIC\",",
                "\"status\":\"{}\",",
                "\"reason_code\":\"{}\",",
                "\"boot_instance_id\":{},",
                "\"booted_deployment_id\":{},",
                "\"current_userspace_deployment_id\":{},",
                "\"target_id\":null,",
                "\"decision_id\":null,",
                "\"challenge_id\":null,",
                "\"effective_policy_digest\":null,",
                "\"artifact_digest\":null,",
                "\"measurement\":null,",
                "\"authenticated\":false,",
                "\"release_qualifying\":false",
                "}}"
            ),
            SCHEMA,
            self.status.as_str(),
            self.reason_code,
            json_option(self.boot_instance_id.as_deref()),
            json_option(self.booted_deployment_id.as_deref()),
            json_option(self.current_userspace_deployment_id.as_deref()),
        )
    }
}

fn json_option(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", json_escape(value)),
        None => "null".to_owned(),
    }
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn read_boot_id(root: &Path) -> Option<String> {
    let value = fs::read_to_string(root.join(BOOT_ID_FILE)).ok()?;
    let value = value.trim();
    if valid_boot_id(value) {
        Some(value.to_owned())
    } else {
        None
    }
}

fn valid_boot_id(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    for (index, byte) in value.bytes().enumerate() {
        if [8, 13, 18, 23].contains(&index) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn resolve_store_link(root: &Path, relative_link: &Path) -> Result<String, &'static str> {
    let canonical_root = fs::canonicalize(root).map_err(|_| "fixture-root-invalid")?;
    let link_path = canonical_root.join(relative_link);
    let target = fs::read_link(&link_path).map_err(|_| "deployment-link-unreadable")?;

    // In fixture mode, absolute targets such as /nix/store/... are mapped
    // beneath the fixture root instead of resolving against the host filesystem.
    let candidate = if target.is_absolute() {
        let relative_target = target
            .strip_prefix("/")
            .map_err(|_| "deployment-target-invalid")?;
        canonical_root.join(relative_target)
    } else {
        link_path
            .parent()
            .ok_or("deployment-link-parent-missing")?
            .join(target)
    };

    let resolved = fs::canonicalize(candidate).map_err(|_| "deployment-target-unresolvable")?;
    let relative = resolved
        .strip_prefix(&canonical_root)
        .map_err(|_| "deployment-target-escapes-root")?;
    let deployment_id = format!("/{}", relative.to_string_lossy().replace('\\', "/"));

    if !valid_nix_store_path(&deployment_id) {
        return Err("deployment-target-not-a-nix-store-path");
    }
    Ok(deployment_id)
}

fn valid_nix_store_path(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("/nix/store/") else {
        return false;
    };
    let Some((hash, name)) = rest.split_once('-') else {
        return false;
    };
    hash.len() == 32
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"abcdfghijklmnpqrsvwxyz".contains(&byte))
        && !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn parse_root_argument() -> Result<PathBuf, &'static str> {
    let mut args = env::args_os().skip(1);
    let mut root: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!("srq-boot-observation-probe [--root FIXTURE_ROOT]");
            println!("Read-only D0 diagnostic; never release-qualifying.");
            std::process::exit(0);
        }
        if arg == "--root" {
            if root.is_some() {
                return Err("duplicate-root-argument");
            }
            root = Some(PathBuf::from(
                args.next().ok_or("root-argument-needs-value")?,
            ));
        } else {
            return Err("unknown-argument");
        }
    }
    Ok(root.unwrap_or_else(|| PathBuf::from("/")))
}

fn main() -> ExitCode {
    let root = match parse_root_argument() {
        Ok(root) => root,
        Err(reason) => {
            eprintln!("srq-boot-observation-probe: {reason}");
            return ExitCode::from(2);
        }
    };

    let record = DiagnosticRecord::evaluate(&root);
    println!("{}", record.to_json());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BOOT_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
    const STORE_HASH_A: &str = "0123456789abcdfghijklmnpqrsvwxyz";
    const STORE_HASH_B: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let root = env::temp_dir().join(format!(
                "srq-boot-observation-probe-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("run")).expect("create run dir");
            fs::create_dir_all(root.join("proc/sys/kernel/random")).expect("create boot-id dir");
            fs::create_dir_all(root.join("nix/store")).expect("create store dir");
            fs::write(root.join(BOOT_ID_FILE), format!("{BOOT_ID}\n")).expect("write boot ID");
            Self { root }
        }

        fn store_path(&self, hash: &str, name: &str) -> PathBuf {
            let path = self.root.join("nix/store").join(format!("{hash}-{name}"));
            fs::create_dir_all(&path).expect("create store path");
            path
        }

        fn point(&self, relative: &str, target: &str) {
            symlink(target, self.root.join(relative)).expect("create deployment symlink");
        }

        fn cleanup(self) {
            fs::remove_dir_all(self.root).expect("remove fixture");
        }
    }

    #[test]
    fn matching_booted_and_current_paths_is_still_d0_only() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-test");
        fixture.point(
            BOOTED_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );
        fixture.point(
            CURRENT_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Match);
        assert_eq!(record.boot_instance_id.as_deref(), Some(BOOT_ID));
        assert_eq!(
            record.booted_deployment_id.as_deref(),
            Some("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test")
        );
        let json = record.to_json();
        assert!(json.contains("\"evidence_tier\":\"D0_LOCAL_DIAGNOSTIC\""));
        assert!(json.contains("\"authenticated\":false"));
        assert!(json.contains("\"release_qualifying\":false"));
        assert!(json.contains("\"challenge_id\":null"));
        fixture.cleanup();
    }

    #[test]
    fn live_userspace_switch_is_reported_as_mismatch_not_a_reboot() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-booted");
        fixture.store_path(STORE_HASH_B, "nixos-system-current");
        fixture.point(
            BOOTED_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-booted",
        );
        fixture.point(
            CURRENT_LINK,
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system-current",
        );

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Mismatch);
        assert_eq!(
            record.reason_code,
            "current-userspace-differs-from-booted-system"
        );
        assert!(!record.to_json().contains("\"release_qualifying\":true"));
        fixture.cleanup();
    }

    #[test]
    fn missing_booted_system_is_blocked_and_not_repaired() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-current");
        fixture.point(
            CURRENT_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-current",
        );
        let before = fs::read_dir(fixture.root.join("run")).unwrap().count();

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Blocked);
        assert_eq!(record.reason_code, "booted-system-missing-or-invalid");
        assert_eq!(
            fs::read_dir(fixture.root.join("run")).unwrap().count(),
            before
        );
        fixture.cleanup();
    }

    #[test]
    fn malformed_boot_id_blocks_observation() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-test");
        fixture.point(
            BOOTED_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );
        fixture.point(
            CURRENT_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );
        fs::write(fixture.root.join(BOOT_ID_FILE), "not-a-boot-id\n").unwrap();

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Blocked);
        assert_eq!(record.reason_code, "boot-id-missing-or-invalid");
        fixture.cleanup();
    }

    #[test]
    fn malformed_store_path_blocks_observation() {
        let fixture = Fixture::new();
        fixture.point(BOOTED_LINK, "/nix/store/not-a-valid-store-path");
        fixture.point(CURRENT_LINK, "/nix/store/not-a-valid-store-path");

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Blocked);
        assert_eq!(record.reason_code, "booted-system-missing-or-invalid");
        fixture.cleanup();
    }

    #[test]
    fn symlink_escape_from_fixture_root_fails_closed() {
        let fixture = Fixture::new();
        fixture.point(BOOTED_LINK, "../../etc/passwd");
        fixture.point(CURRENT_LINK, "../../etc/passwd");

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Blocked);
        assert_eq!(record.reason_code, "booted-system-missing-or-invalid");
        fixture.cleanup();
    }

    #[test]
    fn missing_current_system_is_blocked_even_with_valid_booted_system() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-booted");
        fixture.point(
            BOOTED_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-booted",
        );

        let record = DiagnosticRecord::evaluate(&fixture.root);
        assert_eq!(record.status, Status::Blocked);
        assert_eq!(record.reason_code, "current-system-missing-or-invalid");
        fixture.cleanup();
    }

    #[test]
    fn matching_status_never_populates_target_decision_challenge_or_measurement() {
        let fixture = Fixture::new();
        fixture.store_path(STORE_HASH_A, "nixos-system-test");
        fixture.point(
            BOOTED_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );
        fixture.point(
            CURRENT_LINK,
            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-test",
        );

        let json = DiagnosticRecord::evaluate(&fixture.root).to_json();
        assert!(json.contains("\"target_id\":null"));
        assert!(json.contains("\"decision_id\":null"));
        assert!(json.contains("\"challenge_id\":null"));
        assert!(json.contains("\"measurement\":null"));
        fixture.cleanup();
    }
}
