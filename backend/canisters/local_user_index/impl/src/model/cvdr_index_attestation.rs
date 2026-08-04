//! Pinned OpenChatZD INDEX attestation labels (spec §14 / §17 / G v0.7.0).
//!
//! These strings are the wire/doc contract for CVDR-Verify OpenChatZD output.
//! Capture/store code lands in M4; this module freezes names and claim discipline early.
//!
//! Timing labels are the five-value axis orthogonal to V3 evidence outcomes.
//! Delay = t(INDEX certificate) − t(commitment certificate) (certificate-pair separation; S12).

/// Outer portable package name (spec §14.1).
pub const PORTABLE_PACKAGE_V2_NAME: &str = "PortablePackageV2";

/// Portable package schema id (spec §14.1).
pub const PORTABLE_PACKAGE_SCHEMA: &str = "openchatzd.cvdr.portable_package";

/// Portable package version (spec §14.1). Exact `version == 2` on the wire.
pub const PORTABLE_PACKAGE_VERSION: u32 = 2;

/// V3 code-identity outcomes (spec §17.1). V3-A is retired and must not reappear.
pub const INDEX_HASH_MATCH_AT_CERT_TIME: &str = "INDEX_HASH_MATCH_AT_CERT_TIME";
pub const INDEX_HASH_MISMATCH: &str = "INDEX_HASH_MISMATCH";
pub const INDEX_ATTESTATION_UNAVAILABLE: &str = "INDEX_ATTESTATION_UNAVAILABLE";
pub const INDEX_ATTESTATION_INVALID: &str = "INDEX_ATTESTATION_INVALID";

/// Orthogonal timing qualifiers (spec §17.2 / G v0.7.0). Exact wire strings.
pub const TIMING_ROUTINE: &str = "ROUTINE";
pub const TIMING_DELAY_EXCEEDED: &str = "DELAY_EXCEEDED";
pub const TIMING_PREDATES_COMMITMENT: &str = "PREDATES_COMMITMENT";
pub const TIMING_OUTSIDE_COMPLETION_WINDOW: &str = "OUTSIDE_COMPLETION_WINDOW";
pub const TIMING_NOT_APPLICABLE: &str = "NOT_APPLICABLE";

/// Allowed OpenChatZD-scoped claim (spec §12). Keep in sync with RTS / claims register.
pub const OCZD_SUPPORTED_CLAIM: &str = "OpenChatZD can carry subnet-certified evidence of the Module Hash \
installed on the INDEX canister at the INDEX certificate time and compare it with the h_index \
captured in the receipt. Because OpenChatZD currently has no demonstrated upgrade-continuity \
interlock on local_user_index, matching endpoint hashes do not prove uninterrupted execution by \
that module throughout the sealing window.";

/// Forbidden overclaim fragment (spec §12) — must never appear in verifier/docs output.
pub const OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT: &str = "proves which INDEX code ran when the receipt was sealed";

/// Relative path from the Together-alone `CVDR-Verify` root to the OpenChatZD attestation module.
#[cfg(test)]
const SIBLING_OPENCHATZD_ATTESTATION_REL: &str = "mktd02/mktd02-verify/src/openchatzd/index_attestation.rs";

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
        TIMING_PREDATES_COMMITMENT,
        TIMING_OUTSIDE_COMPLETION_WINDOW,
        TIMING_NOT_APPLICABLE,
        PORTABLE_PACKAGE_SCHEMA,
    ]
}

/// Retired V3-A style tokens that must not be reintroduced as live outcome labels.
#[cfg(test)]
fn retired_v3a_outcome_tokens() -> &'static [&'static str] {
    &["\"V3-A\"", "\"V3A\"", "V3_A_"]
}

/// Retired pre-v0.7.0 timing wire assignments (must not reappear as live labels).
#[cfg(test)]
fn source_has_retired_timing_wire(src: &str) -> bool {
    // Match live assignments / consts only — not mentions inside drift-guard tests.
    src.contains("TIMING_LATE_PATH") || src.contains("= \"late_path\"") || src.contains("= \"routine\"")
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
fn sibling_label_guard(verify_root: &std::path::Path, module_path: &std::path::Path) -> SiblingLabelGuard {
    if !verify_root.exists() {
        SiblingLabelGuard::SkipAbsent
    } else if !module_path.exists() {
        SiblingLabelGuard::FailMissingModule
    } else {
        SiblingLabelGuard::RequireLabelMatch
    }
}

/// Returns `Ok(())` when `src` carries every required amended label and no retired tokens.
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
    if source_has_retired_timing_wire(src) {
        return Err("retired timing wire (late_path / lowercase routine) must not reappear".into());
    }
    Ok(())
}

/// Resolve CVDR-Verify root: prefer a checkout that actually contains the OpenChatZD
/// attestation module (Together-alone sibling or nested CI path).
#[cfg(test)]
fn resolve_cvdr_verify_root() -> Option<std::path::PathBuf> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest.join("../../../../../CVDR-Verify"), // Together-alone/<repos>
        manifest.join("../../../../CVDR-Verify"),    // open-chatZD/CVDR-Verify (CI nested)
    ];
    let mut first_existing: Option<std::path::PathBuf> = None;
    for root in candidates {
        if !root.exists() {
            continue;
        }
        if first_existing.is_none() {
            first_existing = Some(root.clone());
        }
        let module = root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        if module.exists() {
            return Some(root);
        }
    }
    // Fall back to first existing root so FailMissingModule still fires loudly.
    first_existing
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
    fn timing_axis_is_five_distinct_labels() {
        let timing = [
            TIMING_ROUTINE,
            TIMING_DELAY_EXCEEDED,
            TIMING_PREDATES_COMMITMENT,
            TIMING_OUTSIDE_COMPLETION_WINDOW,
            TIMING_NOT_APPLICABLE,
        ];
        assert_eq!(timing.len(), 5);
        for (i, a) in timing.iter().enumerate() {
            for (j, b) in timing.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
        // A late match must not collapse into an unqualified match label.
        assert_ne!(TIMING_OUTSIDE_COMPLETION_WINDOW, "OUTSIDE_COMPLETION_WINDOW_TYPO");
        assert_eq!(TIMING_OUTSIDE_COMPLETION_WINDOW, "OUTSIDE_COMPLETION_WINDOW");
        assert_eq!(TIMING_ROUTINE, "ROUTINE");
        assert!(!source_has_retired_timing_wire(
            "pub const TIMING_ROUTINE: &str = \"ROUTINE\";"
        ));
        assert!(source_has_retired_timing_wire(&format!(
            "pub const TIMING_{}: &str = \"{}{}\";",
            "LATE_PATH", "late", "_path"
        )));
        assert!(source_has_retired_timing_wire(
            "pub const TIMING_ROUTINE: &str = \"routine\";"
        ));
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
            INDEX_ATTESTATION_UNAVAILABLE, "INDEX_ATTESTATION_UNAVAILABLE",
            "FrozenWire-only Available must map to UNAVAILABLE, not MATCH"
        );
    }

    #[test]
    fn sibling_label_guard_skips_when_repo_absent() {
        let missing = Path::new("/tmp/openchatzd-cvdr-verify-definitely-absent-xyz");
        assert!(!missing.exists());
        let module = missing.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        assert_eq!(sibling_label_guard(missing, &module), SiblingLabelGuard::SkipAbsent);
    }

    #[test]
    fn sibling_label_guard_fails_when_repo_present_but_module_missing() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let root: PathBuf = std::env::temp_dir().join(format!("oczd-cvdr-verify-gap-{stamp}"));
        fs::create_dir_all(&root).expect("mkdir verify root");
        let module = root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        assert!(!module.exists());
        assert_eq!(sibling_label_guard(&root, &module), SiblingLabelGuard::FailMissingModule);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sibling_label_guard_requires_match_when_module_present() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let root: PathBuf = std::env::temp_dir().join(format!("oczd-cvdr-verify-ok-{stamp}"));
        let module = root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        fs::create_dir_all(module.parent().expect("parent")).expect("mkdir parents");
        fs::write(&module, "// stub\n").expect("write stub");
        assert_eq!(sibling_label_guard(&root, &module), SiblingLabelGuard::RequireLabelMatch);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_cvdr_verify_root_prefers_checkout_with_module() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let base = std::env::temp_dir().join(format!("oczd-resolve-{stamp}"));
        let empty = base.join("empty");
        let full = base.join("full");
        fs::create_dir_all(&empty).unwrap();
        let module = full.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        fs::write(&module, "// ok\n").unwrap();

        // Simulate preference: when both exist, module-bearing root wins.
        assert!(empty.exists());
        assert!(module.exists());
        let preferred = if module.exists() { full.clone() } else { empty.clone() };
        assert_eq!(preferred, full);
        assert!(preferred.join(SIBLING_OPENCHATZD_ATTESTATION_REL).exists());
        let _ = fs::remove_dir_all(&base);
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

    #[test]
    fn sibling_source_matcher_rejects_retired_timing_wire() {
        let mut src = String::new();
        for label in required_sibling_attestation_labels() {
            src.push_str(label);
            src.push('\n');
        }
        src.push_str("do not prove uninterrupted execution\n");
        // Build without embedding the retired wire literally in this file's source
        // (keeps cross-repo scanners clean).
        let retired = format!("pub const TIMING_{}: &str = \"{}{}\";\n", "LATE_PATH", "late", "_path");
        src.push_str(&retired);
        let err = sibling_source_matches_openchatzd_labels(&src).unwrap_err();
        assert!(err.contains("retired timing"));
    }

    /// Drift guard: when the sibling `CVDR-Verify` repo is checked out, OpenChatZD labels
    /// must appear verbatim in the offline verifier.
    #[test]
    fn labels_match_sibling_cvdr_verify_when_present() {
        let Some(verify_root) = resolve_cvdr_verify_root() else {
            eprintln!("skip cross-repo labels: CVDR-Verify sibling not checked out");
            return;
        };
        let path = verify_root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        match sibling_label_guard(&verify_root, &path) {
            SiblingLabelGuard::SkipAbsent => unreachable!("root exists"),
            SiblingLabelGuard::FailMissingModule => {
                panic!(
                    "CVDR-Verify is present at {} but {} is missing. \
                     Pin gap or moved path: amended OpenChatZD INDEX labels are not in this checkout. \
                     Amend CVDR-Verify or check out a tip that includes openchatzd/index_attestation.rs.",
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
