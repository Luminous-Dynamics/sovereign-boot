//! Standalone, fail-open boot lifecycle helpers.
//!
//! This crate is intentionally independent of the Symthaea monorepo. It writes
//! factual local receipts and atomically maintains Nix GC-root symlinks. It
//! does not authenticate its own JSON, roll back the host, or qualify TPM,
//! kernel-crash, VM-reset, or physical-power-loss behavior.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
const BOOT_STATE_SCHEMA: &str = "spore-boot-state-v1";
const LINEAGE_SCHEMA: &str = "spore-boot-lineage-v1";
const SHUTDOWN_SCHEMA: &str = "spore-shutdown-v1";
const LKG_DECISION_SCHEMA: &str = "lkg-decision-v1";
const NOT_READY_EXIT: i32 = 75;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

type Options = BTreeMap<String, String>;

fn error(message: impl Into<String>) -> String {
    message.into()
}

fn parse_options(args: &[String], boolean_flags: &[&str]) -> Result<Options, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        if !key.starts_with("--") || key.len() < 3 {
            return Err(error(format!("unexpected argument: {key}")));
        }
        if options.contains_key(key) {
            return Err(error(format!("duplicate option: {key}")));
        }
        if boolean_flags.contains(&key) {
            options.insert(key.to_owned(), "true".to_owned());
            index += 1;
            continue;
        }
        index += 1;
        if index >= args.len() || args[index].starts_with("--") {
            return Err(error(format!("option {key} requires a value")));
        }
        options.insert(key.to_owned(), args[index].clone());
        index += 1;
    }
    Ok(options)
}

fn required<'a>(options: &'a Options, key: &str) -> Result<&'a str, String> {
    options
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error(format!("missing required option {key}")))
}

fn optional<'a>(options: &'a Options, key: &str) -> Option<&'a str> {
    options.get(key).map(String::as_str).filter(|value| !value.is_empty())
}

fn parse_u64(options: &Options, key: &str) -> Result<u64, String> {
    required(options, key)?
        .parse::<u64>()
        .map_err(|_| error(format!("option {key} must be a non-negative integer")))
}

fn valid_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_boot_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(error("boot ID is empty or contains unsupported characters"));
    }
    Ok(())
}

fn validate_store_generation(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let components: Vec<_> = path.components().collect();
    if components.len() != 4
        || components[0] != Component::RootDir
        || components[1] != Component::Normal(OsStr::new("nix"))
        || components[2] != Component::Normal(OsStr::new("store"))
    {
        return Err(error("deployment ID must be one canonical /nix/store/<hash>-<name> path"));
    }
    let Some(Component::Normal(name)) = components.get(3) else {
        return Err(error("deployment ID lacks a store object name"));
    };
    let name = name.to_string_lossy();
    let Some(separator) = name.find('-') else {
        return Err(error("deployment store object must contain a hash/name separator"));
    };
    if separator == 0
        || name.len() <= separator + 1
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+._?=-".contains(&byte))
    {
        return Err(error("deployment store object contains invalid characters"));
    }
    Ok(path.to_path_buf())
}

fn require_existing_generation(value: &str) -> Result<PathBuf, String> {
    let path = validate_store_generation(value)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|e| error(format!("deployment does not exist at {value}: {e}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error(format!("deployment is not a real directory: {value}")));
    }
    Ok(path)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|e| error(format!("create directory {}: {e}", path.display())))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| error(format!("inspect directory {}: {e}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error(format!(
            "required directory is not a real directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| error(format!("sync directory {}: {e}", path.display())))
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control <= '\u{1f}' => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| error(format!("path has no parent directory: {}", path.display())))?;
    ensure_directory(parent)?;
    let basename = path
        .file_name()
        .ok_or_else(|| error(format!("path has no file name: {}", path.display())))?
        .to_string_lossy();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{basename}.tmp.{}.{}", std::process::id(), counter));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| error(format!("create temporary file {}: {e}", temporary.display())))?;
        file.write_all(bytes)
            .map_err(|e| error(format!("write temporary file {}: {e}", temporary.display())))?;
        file.sync_all()
            .map_err(|e| error(format!("sync temporary file {}: {e}", temporary.display())))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|e| error(format!("atomically replace {}: {e}", path.display())))?;
        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn read_root_target(root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let link = root.join(name);
    let metadata = match fs::symlink_metadata(&link) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(format!("inspect GC root {}: {e}", link.display()))),
    };
    if !metadata.file_type().is_symlink() {
        return Err(error(format!(
            "GC root exists but is not a symlink: {}",
            link.display()
        )));
    }
    let target = fs::read_link(&link)
        .map_err(|e| error(format!("read GC root {}: {e}", link.display())))?;
    if !target.is_absolute() {
        return Err(error(format!("GC root target must be absolute: {}", link.display())));
    }
    let target_text = target.to_string_lossy();
    let target = validate_store_generation(&target_text)?;
    let target_metadata = fs::symlink_metadata(&target)
        .map_err(|e| error(format!("GC root target does not exist: {}", target.display())))?;
    if target_metadata.file_type().is_symlink() || !target_metadata.is_dir() {
        return Err(error(format!("GC root target is not a real directory: {}", target.display())));
    }
    Ok(Some(target))
}

fn atomic_symlink(root: &Path, name: &str, target: &Path) -> Result<(), String> {
    ensure_directory(root)?;
    let final_path = root.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&final_path) {
        if !metadata.file_type().is_symlink() {
            return Err(error(format!(
                "refusing to replace non-symlink GC root: {}",
                final_path.display()
            )));
        }
    }

    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let scratch_name = format!(".{name}.tmp.{}.{}", std::process::id(), counter);
    let scratch_path = root.join(scratch_name);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &scratch_path)
        .map_err(|e| error(format!("create temporary GC root {}: {e}", scratch_path.display())))?;
    #[cfg(not(unix))]
    return Err(error("GC-root symlink operations require a Unix host"));

    if let Err(e) = fs::rename(&scratch_path, &final_path) {
        let _ = fs::remove_file(&scratch_path);
        return Err(error(format!("atomically replace GC root {}: {e}", final_path.display())));
    }
    sync_directory(root)
}

fn clean_stale_root_scratch(root: &Path) -> Result<(), String> {
    ensure_directory(root)?;
    for entry in fs::read_dir(root)
        .map_err(|e| error(format!("list GC roots {}: {e}", root.display())))?
    {
        let entry = entry.map_err(|e| error(format!("read GC root entry: {e}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let owned_scratch = [".current.tmp.", ".previous.tmp.", ".last-known-good.tmp."]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        if owned_scratch && entry.file_type().map_err(|e| error(e.to_string()))?.is_symlink() {
            fs::remove_file(entry.path())
                .map_err(|e| error(format!("remove stale scratch root {}: {e}", entry.path().display())))?;
        }
    }
    sync_directory(root)
}

fn rotate_root_links(
    root: &Path,
    next_generation: &Path,
    previous_current: Option<&Path>,
) -> Result<(), String> {
    ensure_directory(root)?;
    if let Some(old_generation) = previous_current {
        if old_generation != next_generation {
            atomic_symlink(root, "previous", old_generation)?;
        }
    }
    if previous_current != Some(next_generation) {
        atomic_symlink(root, "current", next_generation)?;
    }
    Ok(())
}

fn rotate_current_previous_roots(root: &Path, next_generation: &Path) -> Result<Option<PathBuf>, String> {
    ensure_directory(root)?;
    let previous_current = read_root_target(root, "current")?;
    rotate_root_links(root, next_generation, previous_current.as_deref())?;
    Ok(previous_current)
}
fn validate_hardware_fingerprint(value: &str) -> Result<(), String> {
    if !valid_hex_digest(value) {
        return Err(error("hardware fingerprint must be a lowercase 64-character SHA-256 digest"));
    }
    Ok(())
}

fn current_boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn shutdown(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, &[])?;
    let state_dir = PathBuf::from(required(&options, "--state-dir")?);
    let runtime_dir = PathBuf::from(required(&options, "--runtime-dir")?);
    let kind = required(&options, "--kind")?;
    if !["reboot", "poweroff", "suspend", "hibernate"].contains(&kind) {
        return Err(error("kind must be reboot, poweroff, suspend, or hibernate"));
    }
    let uptime_secs = parse_u64(&options, "--uptime-secs")?;
    let boot_id = current_boot_id();
    validate_boot_id(&boot_id)?;

    let generation = optional(&options, "--generation");
    if let Some(generation) = generation {
        require_existing_generation(generation)?;
    }
    let hardware = optional(&options, "--hardware-fingerprint");
    if let Some(hardware) = hardware {
        validate_hardware_fingerprint(hardware)?;
    }
    for key in options.keys() {
        if ![
            "--state-dir",
            "--runtime-dir",
            "--kind",
            "--uptime-secs",
            "--generation",
            "--hardware-fingerprint",
        ]
        .contains(&key.as_str())
        {
            return Err(error(format!("unsupported option in shutdown: {key}")));
        }
    }

    ensure_directory(&state_dir)?;
    ensure_directory(&runtime_dir)?;
    let document = format!(
        "{{\n  \"schema\": {},\n  \"clean_shutdown\": true,\n  \"boot_id\": {},\n  \"kind\": {},\n  \"uptime_secs\": {},\n  \"generation\": {},\n  \"hardware_fingerprint\": {}\n}}\n",
        json_string(SHUTDOWN_SCHEMA),
        json_string(&boot_id),
        json_string(kind),
        uptime_secs,
        json_optional_string(generation),
        json_optional_string(hardware),
    );
    atomic_write(&state_dir.join("clean-shutdown.json"), document.as_bytes())?;
    atomic_write(&runtime_dir.join("last-shutdown.json"), document.as_bytes())?;
    Ok(())
}

fn prepare(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, &[])?;
    let roots_dir = PathBuf::from(required(&options, "--roots-dir")?);
    let state_dir = PathBuf::from(required(&options, "--state-dir")?);
    let runtime_dir = PathBuf::from(required(&options, "--runtime-dir")?);
    let generation_text = required(&options, "--generation")?;
    let generation = require_existing_generation(generation_text)?;
    let boot_id = required(&options, "--boot-id")?;
    validate_boot_id(boot_id)?;
    let hardware = required(&options, "--hardware-fingerprint")?;
    validate_hardware_fingerprint(hardware)?;
    let storage_state = required(&options, "--storage-state")?;
    if !["unknown", "healthy", "degraded", "failed"].contains(&storage_state) {
        return Err(error("storage-state must be unknown, healthy, degraded, or failed"));
    }
    for key in options.keys() {
        if ![
            "--roots-dir",
            "--state-dir",
            "--runtime-dir",
            "--generation",
            "--boot-id",
            "--hardware-fingerprint",
            "--storage-state",
        ]
        .contains(&key.as_str())
        {
            return Err(error(format!("unsupported option in prepare: {key}")));
        }
    }

    ensure_directory(&state_dir)?;
    ensure_directory(&runtime_dir)?;
    ensure_directory(&roots_dir)?;
    clean_stale_root_scratch(&roots_dir)?;

    let shutdown_path = state_dir.join("clean-shutdown.json");
    let prior_clean_shutdown = match fs::symlink_metadata(&shutdown_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(error("clean-shutdown.json is not a regular file"));
            }
            fs::remove_file(&shutdown_path)
                .map_err(|e| error(format!("consume previous clean-shutdown marker: {e}")))?;
            sync_directory(&state_dir)?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(error(format!("inspect previous shutdown marker: {e}"))),
    };

    let previous_generation = rotate_current_previous_roots(&roots_dir, &generation)?;
    let generation_string = generation.to_string_lossy();
    let previous_string = previous_generation.as_ref().map(|path| path.to_string_lossy().into_owned());

    let receipt = format!(
        "{{\n  \"schema\": {},\n  \"boot_id\": {},\n  \"booted_generation\": {},\n  \"hardware_fingerprint\": {},\n  \"storage_state\": {},\n  \"generation_source\": \"/run/booted-system\",\n  \"observation_authentication\": \"local-unsigned-diagnostic\"\n}}\n",
        json_string(BOOT_STATE_SCHEMA),
        json_string(boot_id),
        json_string(&generation_string),
        json_string(hardware),
        json_string(storage_state),
    );
    let lineage = format!(
        "{{\n  \"schema\": {},\n  \"boot_id\": {},\n  \"current_generation\": {},\n  \"previous_generation\": {},\n  \"previous_clean_shutdown\": {},\n  \"current_root\": {},\n  \"previous_root\": {},\n  \"lkg_root\": {}\n}}\n",
        json_string(LINEAGE_SCHEMA),
        json_string(boot_id),
        json_string(&generation_string),
        json_optional_string(previous_string.as_deref()),
        prior_clean_shutdown,
        json_string(roots_dir.join("current").to_string_lossy().as_ref()),
        json_string(roots_dir.join("previous").to_string_lossy().as_ref()),
        json_string(roots_dir.join("last-known-good").to_string_lossy().as_ref()),
    );
    atomic_write(&runtime_dir.join("boot-state.json"), receipt.as_bytes())?;
    atomic_write(&runtime_dir.join("lineage.json"), lineage.as_bytes())?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HealthInput {
    booted_generation: String,
    current_generation: String,
    graphical_state: String,
    display_state: String,
    display_pid: u64,
    active_enter_usec: u64,
    observed_usec: u64,
    required_stability_usec: u64,
}

fn not_ready_reason(input: &HealthInput) -> Option<&'static str> {
    if input.booted_generation != input.current_generation {
        return Some("booted-userspace-deployment-mismatch");
    }
    if input.graphical_state != "active" {
        return Some("graphical-target-inactive");
    }
    if input.display_state != "active" {
        return Some("display-manager-inactive");
    }
    if input.display_pid == 0 {
        return Some("display-manager-no-main-pid");
    }
    if input.active_enter_usec == 0 {
        return Some("display-manager-monotonic-start-time-missing");
    }
    if input.observed_usec < input.required_stability_usec {
        return Some("stability-window-not-met");
    }
    None
}

fn write_decision(
    state_dir: &Path,
    report_path: &Path,
    boot_id: &str,
    booted_generation: &str,
    current_generation: &str,
    input: &HealthInput,
    disposition: &str,
    reason: &str,
    resulting_lkg: Option<&str>,
    persistent: bool,
) -> Result<(), String> {
    let document = format!(
        "{{\n  \"schema\": {},\n  \"boot_id\": {},\n  \"booted_generation\": {},\n  \"current_userspace_generation\": {},\n  \"graphical_state\": {},\n  \"display_manager_state\": {},\n  \"display_manager_main_pid\": {},\n  \"display_manager_active_enter_monotonic_usec\": {},\n  \"required_stability_usec\": {},\n  \"observed_stability_usec\": {},\n  \"disposition\": {},\n  \"decision\": {},\n  \"terminal_reason\": {},\n  \"reason_code\": {},\n  \"state_bless_result\": 0,\n  \"resulting_lkg_target\": {}\n}}\n",
        json_string(LKG_DECISION_SCHEMA),
        json_string(boot_id),
        json_string(booted_generation),
        json_string(current_generation),
        json_string(&input.graphical_state),
        json_string(&input.display_state),
        input.display_pid,
        input.active_enter_usec,
        input.required_stability_usec,
        input.observed_usec,
        json_string(disposition),
        json_string(disposition),
        json_string(reason),
        json_string(reason),
        json_optional_string(resulting_lkg),
    );
    atomic_write(report_path, document.as_bytes())?;
    if persistent {
        atomic_write(&state_dir.join("lkg-decision-v1.json"), document.as_bytes())?;
    }
    Ok(())
}

fn qualify(args: &[String]) -> Result<i32, String> {
    let options = parse_options(args, &["--qualification-deadline-expired"])?;
    let roots_dir = PathBuf::from(required(&options, "--roots-dir")?);
    let state_dir = PathBuf::from(required(&options, "--state-dir")?);
    let runtime_dir = PathBuf::from(required(&options, "--runtime-dir")?);
    let boot_id = required(&options, "--boot-id")?;
    validate_boot_id(boot_id)?;
    let booted_text = required(&options, "--booted-generation")?;
    let booted_generation = require_existing_generation(booted_text)?;
    let current_generation = optional(&options, "--current-generation").unwrap_or("").to_owned();
    if !current_generation.is_empty() {
        require_existing_generation(&current_generation)?;
    }
    let input = HealthInput {
        booted_generation: booted_generation.to_string_lossy().into_owned(),
        current_generation: current_generation.clone(),
        graphical_state: required(&options, "--graphical-state")?.to_owned(),
        display_state: required(&options, "--display-state")?.to_owned(),
        display_pid: parse_u64(&options, "--display-pid")?,
        active_enter_usec: parse_u64(&options, "--active-enter-usec")?,
        observed_usec: parse_u64(&options, "--observed-usec")?,
        required_stability_usec: parse_u64(&options, "--required-stability-usec")?,
    };
    let report_path = PathBuf::from(required(&options, "--report-path")?);
    let deadline_expired = options.contains_key("--qualification-deadline-expired");
    for key in options.keys() {
        if ![
            "--roots-dir",
            "--state-dir",
            "--runtime-dir",
            "--boot-id",
            "--booted-generation",
            "--current-generation",
            "--graphical-state",
            "--display-state",
            "--display-pid",
            "--active-enter-usec",
            "--observed-usec",
            "--required-stability-usec",
            "--qualification-deadline-expired",
            "--report-path",
        ]
        .contains(&key.as_str())
        {
            return Err(error(format!("unsupported option in qualify: {key}")));
        }
    }

    ensure_directory(&state_dir)?;
    ensure_directory(&runtime_dir)?;
    ensure_directory(&roots_dir)?;
    let Some(reason) = not_ready_reason(&input) else {
        let existing_lkg = read_root_target(&roots_dir, "last-known-good")?;
        let disposition;
        let terminal_reason;
        let resulting;
        if existing_lkg.as_deref() == Some(booted_generation.as_path()) {
            disposition = "already-known-good";
            terminal_reason = "already-known-good";
            resulting = Some(booted_generation.to_string_lossy().into_owned());
        } else {
            atomic_symlink(&roots_dir, "last-known-good", &booted_generation)?;
            disposition = "promoted";
            terminal_reason = "stable-graphical-boot";
            resulting = Some(booted_generation.to_string_lossy().into_owned());
        }
        write_decision(
            &state_dir,
            &report_path,
            boot_id,
            &input.booted_generation,
            &current_generation,
            &input,
            disposition,
            terminal_reason,
            resulting.as_deref(),
            true,
        )?;
        return Ok(0);
    };

    let existing_lkg = read_root_target(&roots_dir, "last-known-good")?;
    if !deadline_expired {
        write_decision(
            &state_dir,
            &report_path,
            boot_id,
            &input.booted_generation,
            &current_generation,
            &input,
            "pending",
            reason,
            existing_lkg.as_ref().map(|path| path.to_string_lossy()).as_deref(),
            false,
        )?;
        return Ok(NOT_READY_EXIT);
    }

    write_decision(
        &state_dir,
        &report_path,
        boot_id,
        &input.booted_generation,
        &current_generation,
        &input,
        "skipped",
        reason,
        existing_lkg.as_ref().map(|path| path.to_string_lossy()).as_deref(),
        true,
    )?;
    Ok(0)
}

/// Runs the standalone state/clean-shutdown CLI.
pub fn run_state_cli<I, S>(arguments: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = arguments.into_iter().map(Into::into).collect();
    if args.first().is_some_and(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: spore-boot-state shutdown --state-dir DIR --runtime-dir DIR --kind KIND --uptime-secs SECONDS [--generation PATH] [--hardware-fingerprint SHA256]");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("shutdown") => shutdown(&args[1..]),
        _ => Err(error("expected subcommand: shutdown (use --help for usage)")),
    }
}

/// Runs the standalone state/recovery CLI. A result of 75 means health has
/// not converged yet and the systemd timer should try again; it is not failure.
pub fn run_recovery_cli<I, S>(arguments: I) -> Result<i32, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = arguments.into_iter().map(Into::into).collect();
    if args.first().is_some_and(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: spore-recovery-linux {prepare|qualify} ...");
        return Ok(0);
    }
    match args.first().map(String::as_str) {
        Some("prepare") => prepare(&args[1..]).map(|_| 0),
        Some("qualify") => qualify(&args[1..]),
        _ => Err(error("expected subcommand: prepare or qualify (use --help for usage)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
        fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sovereign-boot-lifecycle-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn healthy_input() -> HealthInput {
        HealthInput {
            booted_generation: "/nix/store/00000000000000000000000000000000-test-system".to_owned(),
            current_generation: "/nix/store/00000000000000000000000000000000-test-system".to_owned(),
            graphical_state: "active".to_owned(),
            display_state: "active".to_owned(),
            display_pid: 10,
            active_enter_usec: 123,
            observed_usec: 20_000_000,
            required_stability_usec: 10_000_000,
        }
    }

    #[test]
    fn generation_validator_accepts_canonical_store_objects() {
        assert!(validate_store_generation("/nix/store/00000000000000000000000000000000-test-system").is_ok());
        assert!(validate_store_generation("/nix/store/abc123-nixos-system").is_ok());
    }

    #[test]
    fn generation_validator_rejects_paths_outside_store_and_traversal() {
        for path in [
            "",
            "/tmp/system",
            "/nix/store/../etc",
            "/nix/store/object",
            "/nix/store/-no-hash-name",
            "/nix/store/hash-",
            "/nix/store/hash-name/child",
            "relative/system",
        ] {
            assert!(validate_store_generation(path).is_err(), "{path}");
        }
    }

    #[test]
    fn boot_id_and_fingerprint_validators_reject_bad_values() {
        assert!(validate_boot_id("unknown").is_ok());
        assert!(validate_boot_id("123e4567-e89b-12d3-a456-426614174000").is_ok());
        assert!(validate_boot_id("../etc/passwd").is_err());
        assert!(validate_hardware_fingerprint(&"a".repeat(64)).is_ok());
        assert!(validate_hardware_fingerprint("abcd").is_err());
        assert!(validate_hardware_fingerprint(&"A".repeat(64)).is_err());
    }

    #[test]
    fn json_writer_escapes_quotes_control_characters_and_backslashes() {
        let encoded = json_string("q\"\\\n\t\u{0001}");
        assert!(encoded.starts_with('"') && encoded.ends_with('"'));
        assert!(encoded.contains("\\\""));
        assert!(encoded.contains("\\\\"));
        assert!(encoded.contains("\\n"));
        assert!(encoded.contains("\\t"));
        assert!(encoded.contains("\\u0001"));
    }

    #[test]
    fn option_parser_rejects_duplicates_missing_values_and_unknown_shape() {
        let duplicate = vec!["--x".to_owned(), "1".to_owned(), "--x".to_owned(), "2".to_owned()];
        assert!(parse_options(&duplicate, &[]).is_err());
        let missing = vec!["--x".to_owned()];
        assert!(parse_options(&missing, &[]).is_err());
        let stray = vec!["value".to_owned()];
        assert!(parse_options(&stray, &[]).is_err());
        let boolean = vec!["--flag".to_owned()];
        assert_eq!(parse_options(&boolean, &["--flag"]).unwrap().get("--flag").unwrap(), "true");
    }

    #[test]
    fn atomic_writer_replaces_file_and_leaves_no_temp_files() {
        let root = temp_dir("atomic-write");
        let path = root.join("state.json");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["state.json"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_symlink_swaps_target_and_refuses_regular_file_overwrite() {
        let root = temp_dir("atomic-link");
        let one = root.join("one");
        let two = root.join("two");
        fs::create_dir(&one).unwrap();
        fs::create_dir(&two).unwrap();
        atomic_symlink(&root, "current", &one).unwrap();
        atomic_symlink(&root, "current", &two).unwrap();
        assert_eq!(fs::read_link(root.join("current")).unwrap(), two);
        fs::write(root.join("protected"), "do not overwrite").unwrap();
        assert!(atomic_symlink(&root, "protected", &two).is_err());
        assert_eq!(fs::read_to_string(root.join("protected")).unwrap(), "do not overwrite");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn root_rotation_moves_old_current_to_previous_and_sets_new_current() {
        let root = temp_dir("root-rotation");
        let previous = temp_dir("old-generation");
        let next = temp_dir("new-generation");
        std::os::unix::fs::symlink(&previous, root.join("current")).unwrap();
        let prior = fs::read_link(root.join("current")).unwrap();
        rotate_root_links(&root, &next, Some(&prior)).unwrap();
        assert_eq!(fs::read_link(root.join("previous")).unwrap(), previous);
        assert_eq!(fs::read_link(root.join("current")).unwrap(), next);

        rotate_root_links(&root, &next, Some(&next)).unwrap();
        assert_eq!(fs::read_link(root.join("previous")).unwrap(), previous);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(previous);
        let _ = fs::remove_dir_all(next);
    }

    #[test]
    #[test]
    fn health_reconciliation_requires_actual_boot_match_and_stable_services() {
        let ready = healthy_input();
        assert_eq!(not_ready_reason(&ready), None);

        let mut drift = ready.clone();
        drift.current_generation = "/nix/store/11111111111111111111111111111111-other-system".to_owned();
        assert_eq!(not_ready_reason(&drift), Some("booted-userspace-deployment-mismatch"));

        let mut graphical = ready.clone();
        graphical.graphical_state = "activating".to_owned();
        assert_eq!(not_ready_reason(&graphical), Some("graphical-target-inactive"));

        let mut display = ready.clone();
        display.display_state = "failed".to_owned();
        assert_eq!(not_ready_reason(&display), Some("display-manager-inactive"));

        let mut no_pid = ready.clone();
        no_pid.display_pid = 0;
        assert_eq!(not_ready_reason(&no_pid), Some("display-manager-no-main-pid"));

        let mut no_time = ready.clone();
        no_time.active_enter_usec = 0;
        assert_eq!(not_ready_reason(&no_time), Some("display-manager-monotonic-start-time-missing"));

        let mut too_early = ready;
        too_early.observed_usec = 9_999_999;
        assert_eq!(not_ready_reason(&too_early), Some("stability-window-not-met"));
    }

    #[test]
    fn not_ready_reason_prioritizes_live_switch_over_graphical_health() {
        let mut input = healthy_input();
        input.current_generation = "/nix/store/11111111111111111111111111111111-other-system".to_owned();
        input.display_state = "failed".to_owned();
        assert_eq!(not_ready_reason(&input), Some("booted-userspace-deployment-mismatch"));
    }
}
