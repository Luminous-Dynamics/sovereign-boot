// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-shot Spore install intent used by the relay's prepare/commit ceremony.
//!
//! Large configuration material and the user password are represented only by
//! domain-separated digests.  The relay computes the intent twice: once during
//! prepare and again immediately before destructive execution.  Any changed
//! field, changed disk identity, expired challenge, or replay changes/fails the
//! digest and therefore cannot commit the prepared install.

use serde::{Deserialize, Serialize};

const INSTALL_INTENT_DOMAIN: &[u8] = b"symthaea-spore-install-intent-v1\0";
const CONTENT_DOMAIN: &[u8] = b"symthaea-spore-install-content-v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallIntentFields {
    pub disk: String,
    pub layout: String,
    pub fast_disk: String,
    pub standard_disk: String,
    pub extra_disks: Vec<String>,
    pub hostname: String,
    pub desktop: String,
    pub gpu_driver: String,
    pub timezone: String,
    pub keyboard: String,
    pub secure_boot: bool,
    pub tpm2_unlock: bool,
    pub fido2_unlock: bool,
    pub username: String,
    pub configuration_nix: String,
    pub flake_nix: String,
    pub disko_nix: String,
    pub hardware_nix: String,
    pub user_password: String,
    /// Optional v34 Spore framework execution intent.  Empty means this is a
    /// normal install rather than a framework-bound install.
    pub framework_execution_intent_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallIntentV1 {
    pub schema: String,
    pub challenge: [u8; 32],
    pub disk_identity_digest: [u8; 32],
    pub disk: String,
    pub layout: String,
    pub fast_disk: String,
    pub standard_disk: String,
    pub extra_disks: Vec<String>,
    pub hostname: String,
    pub desktop: String,
    pub gpu_driver: String,
    pub timezone: String,
    pub keyboard: String,
    pub secure_boot: bool,
    pub tpm2_unlock: bool,
    pub fido2_unlock: bool,
    pub username: String,
    pub configuration_nix_digest: [u8; 32],
    pub flake_nix_digest: [u8; 32],
    pub disko_nix_digest: [u8; 32],
    pub hardware_nix_digest: [u8; 32],
    pub user_password_digest: [u8; 32],
    pub framework_execution_intent_sha256: Option<[u8; 32]>,
}

fn content_digest(label: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(CONTENT_DOMAIN);
    hasher.update(label);
    hasher.update(&[0]);
    hasher.update(value);
    *hasher.finalize().as_bytes()
}

fn secret_content_digest(challenge: &[u8; 32], label: &[u8], value: &[u8]) -> [u8; 32] {
    // Salt sensitive low-entropy material with the 256-bit relay-private
    // per-prepare nonce.  The nonce never leaves the relay, so even if a
    // serialized InstallIntentV1 or public fingerprint is retained, it does
    // not expose a stable offline password verifier across preparations.
    let mut hasher = blake3::Hasher::new();
    hasher.update(CONTENT_DOMAIN);
    hasher.update(b"private\0");
    hasher.update(challenge);
    hasher.update(label);
    hasher.update(&[0]);
    hasher.update(value);
    *hasher.finalize().as_bytes()
}

pub fn decode_lower_hex_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "{label} must be 64 lowercase hexadecimal characters"
        ));
    }
    let mut out = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|_| format!("{label} is not UTF-8"))?;
        out[index] = u8::from_str_radix(text, 16)
            .map_err(|_| format!("{label} contains invalid hexadecimal"))?;
    }
    Ok(out)
}

pub fn hex_32(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl InstallIntentV1 {
    pub fn from_fields(
        fields: &InstallIntentFields,
        challenge: [u8; 32],
        disk_identity_digest: [u8; 32],
    ) -> Result<Self, String> {
        let framework_execution_intent_sha256 =
            if fields.framework_execution_intent_sha256.is_empty() {
                None
            } else {
                Some(decode_lower_hex_32(
                    &fields.framework_execution_intent_sha256,
                    "framework_execution_intent_sha256",
                )?)
            };
        Ok(Self {
            schema: "luminous-spore-install-intent-v1".into(),
            challenge,
            disk_identity_digest,
            disk: fields.disk.clone(),
            layout: fields.layout.clone(),
            fast_disk: fields.fast_disk.clone(),
            standard_disk: fields.standard_disk.clone(),
            extra_disks: fields.extra_disks.clone(),
            hostname: fields.hostname.clone(),
            desktop: fields.desktop.clone(),
            gpu_driver: fields.gpu_driver.clone(),
            timezone: fields.timezone.clone(),
            keyboard: fields.keyboard.clone(),
            secure_boot: fields.secure_boot,
            tpm2_unlock: fields.tpm2_unlock,
            fido2_unlock: fields.fido2_unlock,
            username: fields.username.clone(),
            configuration_nix_digest: content_digest(
                b"configuration.nix",
                fields.configuration_nix.as_bytes(),
            ),
            flake_nix_digest: content_digest(b"flake.nix", fields.flake_nix.as_bytes()),
            disko_nix_digest: content_digest(b"disko.nix", fields.disko_nix.as_bytes()),
            hardware_nix_digest: content_digest(
                b"hardware-configuration.nix",
                fields.hardware_nix.as_bytes(),
            ),
            user_password_digest: secret_content_digest(
                &challenge,
                b"user-password",
                fields.user_password.as_bytes(),
            ),
            framework_execution_intent_sha256,
        })
    }

    pub fn digest(&self) -> [u8; 32] {
        let encoded = serde_json::to_vec(self)
            .expect("InstallIntentV1 serialization is infallible for in-memory fields");
        let mut hasher = blake3::Hasher::new();
        hasher.update(INSTALL_INTENT_DOMAIN);
        hasher.update(&encoded);
        *hasher.finalize().as_bytes()
    }

    pub fn fingerprint(&self) -> String {
        self.digest()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> InstallIntentFields {
        InstallIntentFields {
            disk: "/dev/nvme0n1".into(),
            layout: "single".into(),
            fast_disk: String::new(),
            standard_disk: String::new(),
            extra_disks: vec![],
            hostname: "test-host".into(),
            desktop: "plasma".into(),
            gpu_driver: "modesetting".into(),
            timezone: "UTC".into(),
            keyboard: "us".into(),
            secure_boot: true,
            tpm2_unlock: false,
            fido2_unlock: false,
            username: "user".into(),
            configuration_nix: "{ networking.hostName = \"test-host\"; }".into(),
            flake_nix: "{ outputs = _: {}; }".into(),
            disko_nix: String::new(),
            hardware_nix: String::new(),
            user_password: "secret".into(),
            framework_execution_intent_sha256: String::new(),
        }
    }

    #[test]
    fn every_execution_affecting_dimension_changes_digest() {
        let challenge = [7; 32];
        let disk = [8; 32];
        let base = InstallIntentV1::from_fields(&fields(), challenge, disk)
            .unwrap()
            .digest();

        let mut changed = fields();
        changed.configuration_nix.push(' ');
        assert_ne!(
            base,
            InstallIntentV1::from_fields(&changed, challenge, disk)
                .unwrap()
                .digest()
        );

        let mut changed = fields();
        changed.user_password.push('!');
        assert_ne!(
            base,
            InstallIntentV1::from_fields(&changed, challenge, disk)
                .unwrap()
                .digest()
        );

        let mut changed = fields();
        changed.disk = "/dev/sda".into();
        assert_ne!(
            base,
            InstallIntentV1::from_fields(&changed, challenge, disk)
                .unwrap()
                .digest()
        );

        assert_ne!(
            base,
            InstallIntentV1::from_fields(&fields(), [9; 32], disk)
                .unwrap()
                .digest()
        );
        assert_ne!(
            base,
            InstallIntentV1::from_fields(&fields(), challenge, [10; 32])
                .unwrap()
                .digest()
        );
    }

    #[test]
    fn optional_framework_intent_is_strict_lower_hex() {
        let mut value = fields();
        value.framework_execution_intent_sha256 = "aa".repeat(32);
        assert!(InstallIntentV1::from_fields(&value, [1; 32], [2; 32]).is_ok());
        value.framework_execution_intent_sha256 = "GG".repeat(32);
        assert!(InstallIntentV1::from_fields(&value, [1; 32], [2; 32]).is_err());
    }
}
