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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Drift guard: when the sibling CVDR-Verify checkout is present (Together-alone layout),
    /// OpenChatZD labels must appear verbatim in the offline verifier.
    #[test]
    fn labels_match_sibling_cvdr_verify_when_present() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../../CVDR-Verify/mktd02/mktd02-verify/src/openchatzd/index_attestation.rs");
        if !path.exists() {
            eprintln!("skip cross-repo labels: {} not present", path.display());
            return;
        }
        let src = std::fs::read_to_string(&path).expect("read CVDR-Verify index_attestation");
        for label in [
            INDEX_HASH_MATCH_AT_CERT_TIME,
            INDEX_HASH_MISMATCH,
            INDEX_ATTESTATION_UNAVAILABLE,
            INDEX_ATTESTATION_INVALID,
            TIMING_ROUTINE,
            TIMING_DELAY_EXCEEDED,
            TIMING_LATE_PATH,
            PORTABLE_PACKAGE_SCHEMA,
        ] {
            assert!(
                src.contains(label),
                "CVDR-Verify index_attestation.rs missing label `{label}`"
            );
        }
        assert!(src.contains("do not prove uninterrupted execution"));
    }
}
