use chrono::Local;
use sha2::{Digest, Sha256};
use std::fs;
use std::process::Command;
use std::sync::Arc;

use crate::license::{CmdResult, CommandError};

pub type DeviceFingerprint = shared_core::Fingerprint;

/// Policy: what counts as a sufficient hardware observation for binding.
///
/// If an observation fails this check the system must **not** use it to
/// validate or update any binding state — it fails closed.
///
/// Required:
/// - `machine_id` must be present (stable host identifier)
/// - `disk_serial` must be present (independent hardware contributor)
///
/// If any required value is missing the observation is considered
/// partial/insufficient and the caller must propagate an error rather than
/// silently continuing.
pub fn assert_observation_sufficient(obs: &ObservedHardware) -> CmdResult<()> {
    if obs.machine_id.as_deref().map_or(true, |s| s.is_empty()) {
        return Err(CommandError::new(
            "InsufficientObservation",
            "Hardware observation is incomplete: machine_id is required for binding. \
             The system cannot validate or update binding state without a stable machine identifier.",
        ));
    }
    if obs.disk_serial.as_deref().map_or(true, |s| s.is_empty()) {
        return Err(CommandError::new(
            "InsufficientObservation",
            "Hardware observation is incomplete: disk_serial is required for binding. \
             The system cannot validate or update binding state without an independent disk identifier.",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedHardware {
    pub platform: shared_core::Platform,
    pub machine_id: Option<String>,
    pub disk_serial: Option<String>,
    pub cpu_model: Option<String>,
    pub hostname: Option<String>,
    pub locale: Option<String>,
    pub timezone: String,
}

pub trait HardwareObserver: Send + Sync {
    fn observe(&self) -> CmdResult<ObservedHardware>;
}

#[derive(Debug, Clone, Default)]
pub struct SystemHardwareObserver;

impl HardwareObserver for SystemHardwareObserver {
    fn observe(&self) -> CmdResult<ObservedHardware> {
        Ok(ObservedHardware {
            platform: map_platform()?,
            machine_id: collect_machine_id(),
            disk_serial: collect_disk_serial(),
            cpu_model: collect_cpu_model(),
            hostname: whoami::fallible::hostname()
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            locale: std::env::var("LANG")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            timezone: Local::now().format("%z").to_string(),
        })
    }
}

pub fn default_hardware_observer() -> Arc<dyn HardwareObserver + Send + Sync> {
    Arc::new(SystemHardwareObserver)
}

pub fn collect_fingerprint_with_observer(
    installation_id: &str,
    observer: &dyn HardwareObserver,
) -> CmdResult<DeviceFingerprint> {
    let observed = observer.observe()?;
    // Policy: fail closed if observation is partial or insufficient.
    // Do NOT proceed with a weak observation — it could contaminate binding state.
    assert_observation_sufficient(&observed)?;
    collect_fingerprint_from_observation(installation_id, &observed)
}

pub fn collect_fingerprint_from_observation(
    installation_id: &str,
    observed: &ObservedHardware,
) -> CmdResult<DeviceFingerprint> {
    let mut stable = Vec::new();
    stable.push(component(
        shared_core::ComponentKind::InstallationAnchor,
        installation_id,
        shared_core::ComponentSource::Installer,
        4,
    ));

    if let Some(machine_id) = observed.machine_id.as_deref() {
        stable.push(component(
            shared_core::ComponentKind::MachineId,
            machine_id,
            shared_core::ComponentSource::System,
            5,
        ));
    }

    if let Some(disk_serial) = observed.disk_serial.as_deref() {
        stable.push(component(
            shared_core::ComponentKind::DiskSerial,
            disk_serial,
            shared_core::ComponentSource::System,
            4,
        ));
    }

    if let Some(cpu_model) = observed.cpu_model.as_deref() {
        stable.push(component(
            shared_core::ComponentKind::CpuModel,
            cpu_model,
            shared_core::ComponentSource::System,
            3,
        ));
    }

    if let Some(hostname) = observed.hostname.as_deref() {
        stable.push(component(
            shared_core::ComponentKind::Hostname,
            hostname,
            shared_core::ComponentSource::System,
            2,
        ));
    }

    sort_components(&mut stable);

    let mut observations = Vec::new();
    if let Some(locale) = observed.locale.as_ref() {
        observations.push(shared_core::FingerprintObservation {
            kind: shared_core::ObservationKind::Locale,
            value: locale.clone(),
        });
    }
    observations.push(shared_core::FingerprintObservation {
        kind: shared_core::ObservationKind::Timezone,
        value: observed.timezone.clone(),
    });
    sort_observations(&mut observations);

    let mut fingerprint = shared_core::Fingerprint {
        version: 2,
        platform: observed.platform,
        hardware_hash: String::new(),
        binding: shared_core::Binding {
            stable,
            strict: Vec::new(),
            observations,
        },
    };
    fingerprint.hardware_hash = shared_core::recompute_hardware_hash(&fingerprint)
        .map_err(|err| CommandError::parse(err.to_string()))?;
    shared_core::validate_fingerprint(&fingerprint)
        .map_err(|err| CommandError::parse(err.to_string()))?;
    Ok(fingerprint)
}

pub fn fingerprint_hardware_hash_bytes(fingerprint: &DeviceFingerprint) -> CmdResult<[u8; 32]> {
    decode_hash_hex(&fingerprint.hardware_hash)
}

fn map_platform() -> CmdResult<shared_core::Platform> {
    match std::env::consts::OS {
        "macos" => Ok(shared_core::Platform::Macos),
        "linux" => Ok(shared_core::Platform::Linux),
        "windows" => Ok(shared_core::Platform::Windows),
        other => Err(CommandError::parse(format!("unsupported platform {other}"))),
    }
}

fn component(
    kind: shared_core::ComponentKind,
    raw_value: &str,
    source: shared_core::ComponentSource,
    weight: u8,
) -> shared_core::FingerprintComponent {
    shared_core::FingerprintComponent {
        kind,
        hash: hash_normalized(raw_value),
        weight,
        source,
    }
}

fn hash_normalized(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

fn decode_hash_hex(input: &str) -> CmdResult<[u8; 32]> {
    let decoded = hex::decode(input).map_err(|err| CommandError::parse(err.to_string()))?;
    if decoded.len() != 32 {
        return Err(CommandError::parse("hardware hash must be 32 bytes"));
    }
    let mut array = [0u8; 32];
    array.copy_from_slice(&decoded);
    Ok(array)
}

fn sort_components(components: &mut [shared_core::FingerprintComponent]) {
    components.sort_by(|left, right| {
        (
            left.kind.as_str(),
            left.hash.as_str(),
            left.source.as_str(),
            left.weight,
        )
            .cmp(&(
                right.kind.as_str(),
                right.hash.as_str(),
                right.source.as_str(),
                right.weight,
            ))
    });
}

fn sort_observations(observations: &mut [shared_core::FingerprintObservation]) {
    observations.sort_by(|left, right| {
        (left.kind.as_str(), left.value.as_str()).cmp(&(right.kind.as_str(), right.value.as_str()))
    });
}

fn collect_machine_id() -> Option<String> {
    match std::env::consts::OS {
        "linux" => read_first_non_empty(&["/etc/machine-id", "/var/lib/dbus/machine-id"]),
        "macos" => command_output("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])
            .and_then(|output| parse_quoted_assignment(&output, "IOPlatformUUID")),
        "windows" => command_output(
            "reg",
            &[
                "query",
                r"HKLM\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ],
        )
        .and_then(|output| {
            output
                .lines()
                .find(|line| line.contains("MachineGuid"))
                .and_then(|line| {
                    line.split_whitespace()
                        .last()
                        .map(|value| value.to_string())
                })
        }),
        _ => None,
    }
}

fn collect_cpu_model() -> Option<String> {
    match std::env::consts::OS {
        "linux" => fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|contents| {
                contents
                    .lines()
                    .find_map(|line| line.split_once(':'))
                    .and_then(|(key, value)| {
                        if key.trim() == "model name" {
                            Some(value.trim().to_string())
                        } else {
                            None
                        }
                    })
            }),
        "macos" => command_output("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "windows" => std::env::var("PROCESSOR_IDENTIFIER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        _ => None,
    }
    .or_else(|| Some(std::env::consts::ARCH.to_string()))
}

fn collect_disk_serial() -> Option<String> {
    match std::env::consts::OS {
        "linux" => collect_linux_disk_serial(),
        "macos" => collect_macos_disk_serial(),
        "windows" => collect_windows_disk_serial(),
        _ => None,
    }
}

fn collect_linux_disk_serial() -> Option<String> {
    command_output("findmnt", &["-n", "-o", "SOURCE", "/"])
        .and_then(|source| command_output("lsblk", &["-ndo", "SERIAL", source.trim()]))
        .and_then(|output| first_nonempty_line(&output))
        .or_else(|| {
            command_output("lsblk", &["-ndo", "SERIAL"])
                .and_then(|output| first_nonempty_line(&output))
        })
}

fn collect_macos_disk_serial() -> Option<String> {
    command_output(
        "system_profiler",
        &["SPNVMeDataType", "SPSerialATADataType", "SPStorageDataType"],
    )
    .and_then(|output| parse_colon_assignment(&output, "Serial Number"))
    .or_else(|| {
        command_output("ioreg", &["-r", "-c", "AppleAHCIDiskDriver", "-d", "2"])
            .and_then(|output| parse_quoted_assignment(&output, "Serial Number"))
    })
}

fn collect_windows_disk_serial() -> Option<String> {
    command_output("wmic", &["diskdrive", "get", "serialnumber"])
        .and_then(|output| first_nonempty_line(&output))
        .filter(|line| !line.eq_ignore_ascii_case("serialnumber"))
}

fn read_first_non_empty(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        fs::read_to_string(path)
            .ok()
            .map(|contents| contents.trim().to_string())
            .filter(|contents| !contents.is_empty())
    })
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_quoted_assignment(output: &str, key: &str) -> Option<String> {
    output.lines().find_map(|line| {
        if !line.contains(key) {
            return None;
        }
        let parts = line.split('"').collect::<Vec<_>>();
        if parts.len() >= 4 {
            Some(parts[3].trim().to_string())
        } else {
            None
        }
    })
}

fn parse_colon_assignment(output: &str, key: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        if label.trim().eq_ignore_ascii_case(key) {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    })
}

fn first_nonempty_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        assert_observation_sufficient, collect_fingerprint_from_observation,
        collect_fingerprint_with_observer, fingerprint_hardware_hash_bytes, HardwareObserver,
        ObservedHardware,
    };
    use crate::license::CmdResult;

    #[derive(Clone)]
    struct FixedObserver(ObservedHardware);

    impl HardwareObserver for FixedObserver {
        fn observe(&self) -> CmdResult<ObservedHardware> {
            Ok(self.0.clone())
        }
    }

    fn sample_observed(
        machine_id: &str,
        disk_serial: &str,
        cpu: &str,
        hostname: &str,
    ) -> ObservedHardware {
        ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some(machine_id.into()),
            disk_serial: Some(disk_serial.into()),
            cpu_model: Some(cpu.into()),
            hostname: Some(hostname.into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }
    }

    fn sample_no_machine_id() -> ObservedHardware {
        ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: None,
            disk_serial: Some("disk-a".into()),
            cpu_model: Some("AppleM1".into()),
            hostname: Some("host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }
    }

    fn sample_no_disk_serial() -> ObservedHardware {
        ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some("machine-a".into()),
            disk_serial: None,
            cpu_model: Some("AppleM1".into()),
            hostname: Some("host".into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }
    }

    // ── Observer sufficiency policy ────────────────────────────────────────────

    #[test]
    fn observation_without_machine_id_is_insufficient() {
        let obs = sample_no_machine_id();
        let err = assert_observation_sufficient(&obs).unwrap_err();
        assert_eq!(err.code, "InsufficientObservation");
    }

    #[test]
    fn collect_fingerprint_with_observer_fails_when_machine_id_missing() {
        let observer = FixedObserver(sample_no_machine_id());
        let err = collect_fingerprint_with_observer("some-id", &observer).unwrap_err();
        assert_eq!(err.code, "InsufficientObservation");
    }

    #[test]
    fn observation_without_disk_serial_is_insufficient() {
        let obs = sample_no_disk_serial();
        let err = assert_observation_sufficient(&obs).unwrap_err();
        assert_eq!(err.code, "InsufficientObservation");
    }

    #[test]
    fn collect_fingerprint_with_observer_fails_when_disk_serial_missing() {
        let observer = FixedObserver(sample_no_disk_serial());
        let err = collect_fingerprint_with_observer("some-id", &observer).unwrap_err();
        assert_eq!(err.code, "InsufficientObservation");
    }

    #[test]
    fn collect_fingerprint_with_observer_does_not_contaminate_state_on_insufficient_obs() {
        // This test verifies the fail-closed property:
        // when collect_fingerprint_with_observer returns an error, no fingerprint was produced.
        let observer = FixedObserver(sample_no_machine_id());
        let result = collect_fingerprint_with_observer("some-id", &observer);
        // Must be an error — no fingerprint should be produced from a weak observation.
        assert!(result.is_err());
    }

    #[test]
    fn observation_with_machine_id_is_sufficient() {
        let obs = sample_observed("machine-a", "disk-a", "cpu-a", "host-a");
        assert!(assert_observation_sufficient(&obs).is_ok());
    }

    // ── Existing fingerprint tests ─────────────────────────────────────────────

    #[test]
    fn builds_valid_fingerprint_v2_from_observation() {
        let fingerprint = collect_fingerprint_from_observation(
            "550e8400-e29b-41d4-a716-446655440000",
            &sample_observed("machine-a", "disk-a", "cpu-a", "host-a"),
        )
        .expect("collect fingerprint");
        shared_core::validate_fingerprint(&fingerprint).expect("valid fingerprint");
        assert_eq!(
            fingerprint_hardware_hash_bytes(&fingerprint)
                .expect("hardware hash")
                .len(),
            32
        );
    }

    #[test]
    fn fingerprint_is_shared_core_compatible() {
        let fingerprint = collect_fingerprint_from_observation(
            "550e8400-e29b-41d4-a716-446655440000",
            &sample_observed("machine-a", "disk-a", "cpu-a", "host-a"),
        )
        .expect("collect fingerprint");
        let bytes = shared_core::canonical_fingerprint_bytes(&fingerprint).expect("canonical");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn observed_hardware_changes_hash() {
        let first = sample_observed("machine-a", "disk-a", "cpu-a", "host-a");
        let second = sample_observed("machine-b", "disk-a", "cpu-a", "host-a");
        let first_fp =
            collect_fingerprint_from_observation("550e8400-e29b-41d4-a716-446655440000", &first)
                .expect("first fingerprint");
        let second_fp =
            collect_fingerprint_from_observation("550e8400-e29b-41d4-a716-446655440000", &second)
                .expect("second fingerprint");
        assert_ne!(first_fp.hardware_hash, second_fp.hardware_hash);
    }

    #[test]
    fn observer_roundtrip_matches_direct_build() {
        let observed = sample_observed("machine-a", "disk-a", "cpu-a", "host-a");
        let observer = FixedObserver(observed.clone());
        let via_observer =
            collect_fingerprint_with_observer("550e8400-e29b-41d4-a716-446655440000", &observer)
                .expect("observer fingerprint");
        let direct =
            collect_fingerprint_from_observation("550e8400-e29b-41d4-a716-446655440000", &observed)
                .expect("direct fingerprint");
        assert_eq!(via_observer, direct);
    }

    #[test]
    fn fingerprint_includes_disk_serial_component() {
        let fingerprint = collect_fingerprint_from_observation(
            "550e8400-e29b-41d4-a716-446655440000",
            &sample_observed("machine-a", "disk-a", "cpu-a", "host-a"),
        )
        .expect("collect fingerprint");
        assert!(fingerprint
            .binding
            .stable
            .iter()
            .any(|component| { component.kind == shared_core::ComponentKind::DiskSerial }));
    }
}
