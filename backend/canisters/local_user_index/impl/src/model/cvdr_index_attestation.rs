//! Pinned OpenChatZD INDEX attestation labels (spec §14 / §17).
//!
//! These strings are the wire/doc contract for CVDR-Verify OpenChatZD output.
//! Capture/store code lands in M4; this module freezes names and claim discipline early.

/// Outer portable package name (spec §14.1).
pub const PORTABLE_PACKAGE_V2_NAME: &str = "PortablePackageV2";

/// Portable package schema id (spec §14.1).
pub const PORTABLE_PACKAGE_SCHEMA: &str = "openchatzd.cvdr.portable_package";

/// Portable package version (spec §14.1).
pub const PORTABLE_PACKAGE_VERSION: u32 = 2;

/// V3 code-identity outcomes (spec §17.1). V3-A is retired and must not reappear.
pub const INDEX_HASH_MATCH_AT_CERT_TIME: &str = "INDEX_HASH_MATCH_AT_CERT_TIME";
pub const INDEX_HASH_MISMATCH: &str = "INDEX_HASH_MISMATCH";
pub const INDEX_ATTESTATION_UNAVAILABLE: &str = "INDEX_ATTESTATION_UNAVAILABLE";
pub const INDEX_ATTESTATION_INVALID: &str = "INDEX_ATTESTATION_INVALID";

/// Orthogonal timing qualifiers (spec §17.2).
pub const TIMING_ROUTINE: &str = "routine";
pub const TIMING_DELAY_EXCEEDED: &str = "DELAY_EXCEEDED";
pub const TIMING_LATE_PATH: &str = "late_path";

/// Allowed OpenChatZD-scoped claim (spec §12). Keep in sync with RTS / claims register.
pub const OCZD_SUPPORTED_CLAIM: &str = "OpenChatZD can carry subnet-certified evidence of the Module Hash \
installed on the INDEX canister at the INDEX certificate time and compare it with the h_index \
captured in the receipt. Because OpenChatZD currently has no demonstrated upgrade-continuity \
interlock on local_user_index, matching endpoint hashes do not prove uninterrupted execution by \
that module throughout the sealing window.";

/// Forbidden overclaim fragment (spec §12) — must never appear in verifier/docs output.
pub const OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT: &str =
    "proves which INDEX code ran when the receipt was sealed";

/// Relative path from the Together-alone `CVDR-Verify` root to the OpenChatZD attestation module.
#[cfg(test)]
const SIBLING_OPENCHATZD_ATTESTATION_REL: &str =
    "mktd02/mktd02-verify/src/openchatzd/index_attestation.rs";

/// Labels that must appear verbatim in the sibling offline verifier when that module exists.
#[cfg(test)]
fn required_sibling_attestation_labels() -> &'static [&'static str] {
    &[
        INDEX_HASH_MATCH_AT_CERT_TIME,
        INDEX_HASH_MISMATCH,
        INDEX_ATTESTATION_UNAVAILABLE,
        INDEX_ATTESTATION_INVALID,
        TIMING_ROUTINE,
        TIMING_DELAY_EXCEEDED,
        TIMING_LATE_PATH,
        PORTABLE_PACKAGE_SCHEMA,
    ]
}

/// Retired V3-A style tokens that must not be reintroduced as live outcome labels.
#[cfg(test)]
fn retired_v3a_outcome_tokens() -> &'static [&'static str] {
    &["\"V3-A\"", "\"V3A\"", "V3_A_"]
}

/// Decision for the cross-repo label drift guard (pure; unit-tested).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SiblingLabelGuard {
    /// Sibling repo not checked out; cross-repo check inapplicable.
    SkipAbsent,
    /// Repo present but attestation module missing (pin gap / moved path). Must fail loud.
    FailMissingModule,
    /// Module present; caller must read and check labels.
    RequireLabelMatch,
}

#[cfg(test)]
fn sibling_label_guard(
    verify_root: &std::path::Path,
    module_path: &std::path::Path,
) -> SiblingLabelGuard {
    if !verify_root.exists() {
        SiblingLabelGuard::SkipAbsent
    } else if !module_path.exists() {
        SiblingLabelGuard::FailMissingModule
    } else {
        SiblingLabelGuard::RequireLabelMatch
    }
}

/// Returns `Ok(())` when `src` carries every required amended label and no retired V3-A tokens.
#[cfg(test)]
fn sibling_source_matches_openchatzd_labels(src: &str) -> Result<(), String> {
    for label in required_sibling_attestation_labels() {
        if !src.contains(label) {
            return Err(format!("missing required label `{label}`"));
        }
    }
    if !src.contains("do not prove uninterrupted execution") {
        return Err("missing continuity non-claim fragment".into());
    }
    for token in retired_v3a_outcome_tokens() {
        if src.contains(token) {
            return Err(format!("retired V3-A token `{token}` must not reappear"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn portable_package_identity_is_pinned() {
        assert_eq!(PORTABLE_PACKAGE_V2_NAME, "PortablePackageV2");
        assert_eq!(PORTABLE_PACKAGE_SCHEMA, "openchatzd.cvdr.portable_package");
        assert_eq!(PORTABLE_PACKAGE_VERSION, 2);
    }

    #[test]
    fn v3_outcomes_are_distinct_and_complete() {
        let outcomes = [
            INDEX_HASH_MATCH_AT_CERT_TIME,
            INDEX_HASH_MISMATCH,
            INDEX_ATTESTATION_UNAVAILABLE,
            INDEX_ATTESTATION_INVALID,
        ];
        for (i, a) in outcomes.iter().enumerate() {
            for (j, b) in outcomes.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
        assert!(!outcomes.iter().any(|o| o.contains("V3-A") || o.contains("V3A")));
    }

    #[test]
    fn mismatch_is_not_absence() {
        assert_ne!(INDEX_HASH_MISMATCH, INDEX_ATTESTATION_UNAVAILABLE);
        assert_ne!(INDEX_HASH_MISMATCH, INDEX_ATTESTATION_INVALID);
    }

    #[test]
    fn timing_qualifiers_are_orthogonal_labels() {
        assert_ne!(TIMING_ROUTINE, TIMING_DELAY_EXCEEDED);
        assert_ne!(TIMING_DELAY_EXCEEDED, TIMING_LATE_PATH);
        // A late match must not collapse into an unqualified match label.
        assert!(!INDEX_HASH_MATCH_AT_CERT_TIME.contains("DELAY"));
        assert!(!INDEX_HASH_MATCH_AT_CERT_TIME.contains("late"));
    }

    #[test]
    fn supported_claim_rejects_continuity_overclaim() {
        assert!(OCZD_SUPPORTED_CLAIM.contains("do not prove uninterrupted execution"));
        assert!(!OCZD_SUPPORTED_CLAIM.contains(OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT));
        assert!(OCZD_SUPPORTED_CLAIM.contains("no demonstrated upgrade-continuity interlock"));
    }

    #[test]
    fn serving_shapes_remain_independent_labels() {
        // Commitment Available (FrozenWire) vs INDEX Available (PortablePackageV2) are distinct.
        assert_ne!(PORTABLE_PACKAGE_V2_NAME, "FrozenWire");
        assert_eq!(
            INDEX_ATTESTATION_UNAVAILABLE,
            "INDEX_ATTESTATION_UNAVAILABLE",
            "FrozenWire-only Available must map to UNAVAILABLE, not MATCH"
        );
    }

    #[test]
    fn sibling_label_guard_skips_when_repo_absent() {
        let missing = Path::new("/tmp/openchatzd-cvdr-verify-definitely-absent-xyz");
        assert!(!missing.exists());
        let module = missing.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        assert_eq!(
            sibling_label_guard(missing, &module),
            SiblingLabelGuard::SkipAbsent
        );
    }

    #[test]
    fn sibling_label_guard_fails_when_repo_present_but_module_missing() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root: PathBuf = std::env::temp_dir().join(format!("oczd-cvdr-verify-gap-{stamp}"));
        fs::create_dir_all(&root).expect("mkdir verify root");
        let module = root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        assert!(!module.exists());
        assert_eq!(
            sibling_label_guard(&root, &module),
            SiblingLabelGuard::FailMissingModule
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sibling_label_guard_requires_match_when_module_present() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root: PathBuf = std::env::temp_dir().join(format!("oczd-cvdr-verify-ok-{stamp}"));
        let module = root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        fs::create_dir_all(module.parent().expect("parent")).expect("mkdir parents");
        fs::write(&module, "// stub\n").expect("write stub");
        assert_eq!(
            sibling_label_guard(&root, &module),
            SiblingLabelGuard::RequireLabelMatch
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sibling_source_matcher_accepts_complete_amended_labels() {
        let mut src = String::new();
        for label in required_sibling_attestation_labels() {
            src.push_str(label);
            src.push('\n');
        }
        src.push_str("do not prove uninterrupted execution\n");
        assert!(sibling_source_matches_openchatzd_labels(&src).is_ok());
    }

    #[test]
    fn sibling_source_matcher_rejects_missing_label() {
        let src = format!(
            "{}\n{}\ndo not prove uninterrupted execution\n",
            INDEX_HASH_MATCH_AT_CERT_TIME, INDEX_HASH_MISMATCH
        );
        let err = sibling_source_matches_openchatzd_labels(&src).unwrap_err();
        assert!(err.contains("missing required label"));
    }

    #[test]
    fn sibling_source_matcher_rejects_retired_v3a_token() {
        let mut src = String::new();
        for label in required_sibling_attestation_labels() {
            src.push_str(label);
            src.push('\n');
        }
        src.push_str("do not prove uninterrupted execution\n");
        src.push_str("outcome = \"V3-A\"\n");
        let err = sibling_source_matches_openchatzd_labels(&src).unwrap_err();
        assert!(err.contains("retired V3-A"));
    }

    /// Drift guard: when the sibling `CVDR-Verify` repo is checked out (Together-alone layout),
    /// OpenChatZD labels must appear verbatim in the offline verifier.
    ///
    /// If the sibling repo root exists but the OpenChatZD attestation module is missing
    /// (e.g. pin `v0.6.1` predates amended INDEX labels), this test **fails**. It must not
    /// silently pass that pin gap. Skip only when `CVDR-Verify` is not checked out at all.
    #[test]
    fn labels_match_sibling_cvdr_verify_when_present() {
        let verify_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../CVDR-Verify");
        let path = verify_root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        match sibling_label_guard(&verify_root, &path) {
            SiblingLabelGuard::SkipAbsent => {
                eprintln!(
                    "skip cross-repo labels: CVDR-Verify sibling not checked out at {}",
                    verify_root.display()
                );
            }
            SiblingLabelGuard::FailMissingModule => {
                panic!(
                    "CVDR-Verify is present at {} but {} is missing. \
                     Pin gap or moved path: amended OpenChatZD INDEX labels are not in this checkout \
                     (v0.6.1 predates PortablePackageV2 / INDEX attestation). \
                     Amend CVDR-Verify or check out a tip that includes openchatzd/index_attestation.rs. \
                     This failure is expected against pin v0.6.1 and is not a canister regression.",
                    verify_root.display(),
                    path.display()
                );
            }
            SiblingLabelGuard::RequireLabelMatch => {
                let src = fs::read_to_string(&path).expect("read CVDR-Verify index_attestation");
                sibling_source_matches_openchatzd_labels(&src).unwrap_or_else(|e| {
                    panic!("CVDR-Verify OpenChatZD label drift: {e}");
                });
            }
        }
    }
}
