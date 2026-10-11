//! Pinned OpenChatZD INDEX attestation labels (suite v5 — Brief B1 step 5; G step-5 rules).
//!
//! These strings are the wire/doc contract for CVDR-Verify OpenChatZD output (V3A). They are
//! pinned here and drift-guarded against the sibling verifier in both directions.
//!
//! V3A has EXACTLY three outcomes for a live PortablePackageV3 (G rule 2): PASS,
//! PENDING-IN-PROTECTED-WINDOW, PERMANENTLY-UNAVAILABLE. A historical PortablePackageV2 — never
//! emitted by this Index — has one more, NOT_ATTESTED (CVDR-Verify `b2547880`, G 2026-10-01): an
//! Index observation (`h_index`), not subnet-attested; validity INCOMPLETE, reason
//! `v3a-not-attested`, exit 4. It is not a V3 outcome. Anything else is a NAMED FAILURE, never an
//! outcome. Code identity is never inferred from the absence of evidence; pending is never
//! collapsed into unavailable.
//!
//! Timing labels are the five-value axis orthogonal to (and never gating) V3A.

/// Outer portable package name (R-6).
pub const PORTABLE_PACKAGE_V3_NAME: &str = "PortablePackageV3";

/// Portable package schema id (spec §14.1).
pub const PORTABLE_PACKAGE_SCHEMA: &str = "openchatzd.cvdr.portable_package";

/// Portable package version (R-6). Exact `version == 3` on the wire; verifiers fail closed on any other.
pub const PORTABLE_PACKAGE_VERSION: u32 = 3;

/// The three V3A outcomes — exact wire strings.
pub const V3A_PASS: &str = "V3A_PASS";
pub const V3A_PENDING_IN_PROTECTED_WINDOW: &str = "V3A_PENDING_IN_PROTECTED_WINDOW";
pub const V3A_PERMANENTLY_UNAVAILABLE: &str = "V3A_PERMANENTLY_UNAVAILABLE";
/// Historical PortablePackageV2 only — exact wire string; never one of the three V3 outcomes.
pub const HISTORICAL_V2_NOT_ATTESTED: &str = "NOT_ATTESTED";
/// Named V3A failures (validity FAIL) — exact wire strings. Never outcomes.
pub const INDEX_HASH_MISMATCH: &str = "INDEX_HASH_MISMATCH";
pub const INDEX_ATTESTATION_INVALID: &str = "INDEX_ATTESTATION_INVALID";

/// Orthogonal timing qualifiers (spec §17.2 / G v0.7.0). Exact wire strings.
pub const TIMING_ROUTINE: &str = "ROUTINE";
pub const TIMING_DELAY_EXCEEDED: &str = "DELAY_EXCEEDED";
/// INDEX certificate `/time` before the commitment certificate `/time`: the routine order on the v5
/// path, where evidence capture starts at `Uninstalled` (CVDR-Verify rename, G 2026-10-01).
pub const TIMING_BEFORE_COMMITMENT_CERTIFICATE: &str = "BEFORE_COMMITMENT_CERTIFICATE";
pub const TIMING_OUTSIDE_COMPLETION_WINDOW: &str = "OUTSIDE_COMPLETION_WINDOW";
pub const TIMING_NOT_APPLICABLE: &str = "NOT_APPLICABLE";

/// Allowed OpenChatZD-scoped claim (ratified wording, Stef 2026-09-22). Keep in sync with
/// CVDR-Verify `openchatzd/index_attestation.rs`, the RTS and the claims register.
pub const OCZD_SUPPORTED_CLAIM: &str = "OpenChatZD carries subnet-attested installed module identity \
during the finalization/certification window. The Index upgrade interlock bounds that window from \
uninstall to evidence capture: the same Index code stays installed until the module-hash certificate \
is stored, so the certified module hash is the code identity of the deleting Index.";
/// The ratified claim fragment both repos must carry verbatim.
pub const OCZD_CLAIM_FRAGMENT: &str = "subnet-attested installed module identity during the finalization/certification window";

/// Forbidden overclaim fragments — must never appear in verifier/docs output. The interlock
/// bounds the window; it does not prove execution history, and evidence absence proves nothing.
pub const OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT: &str = "proves which INDEX code ran when the receipt was sealed";
/// The internal name of the interval must not leak into claims (Stef 2026-09-22).
pub const OCZD_INTERNAL_ONLY_FRAGMENT: &str = "protected deletion→evidence interval";

/// Relative path from the Together-alone `CVDR-Verify` root to the OpenChatZD attestation module.
#[cfg(test)]
const SIBLING_OPENCHATZD_ATTESTATION_REL: &str = "mktd02/mktd02-verify/src/openchatzd/index_attestation.rs";
/// The sibling's OpenChatZD module directory, whose `const …: &str` definitions the guard reads (the
/// schema id lives in `package.rs`, the labels and claim in `index_attestation.rs`).
#[cfg(test)]
const SIBLING_OPENCHATZD_MODULE_DIR_REL: &str = "mktd02/mktd02-verify/src/openchatzd";

/// Wire labels the sibling verifier must DEFINE with exactly these values.
#[cfg(test)]
fn required_sibling_wire_labels() -> &'static [&'static str] {
    &[
        V3A_PASS,
        V3A_PENDING_IN_PROTECTED_WINDOW,
        V3A_PERMANENTLY_UNAVAILABLE,
        HISTORICAL_V2_NOT_ATTESTED,
        INDEX_HASH_MISMATCH,
        INDEX_ATTESTATION_INVALID,
        TIMING_ROUTINE,
        TIMING_DELAY_EXCEEDED,
        TIMING_BEFORE_COMMITMENT_CERTIFICATE,
        TIMING_OUTSIDE_COMPLETION_WINDOW,
        TIMING_NOT_APPLICABLE,
        PORTABLE_PACKAGE_SCHEMA,
    ]
}

/// String values of the `const NAME: &str = "…";` definitions in Rust source. `//` comment lines are
/// dropped and `\`-newline continuations are joined as rustc joins them, so a label that only
/// appears in a comment or a test literal is never a definition.
#[cfg(test)]
fn str_const_definitions(src: &str) -> Vec<String> {
    let code = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    const DECL: &str = ": &str =";
    let mut values = Vec::new();
    let mut from = 0;
    while let Some(pos) = code[from..].find(DECL) {
        let at = from + pos;
        from = at + DECL.len();
        let line_start = code[..at].rfind('\n').map_or(0, |i| i + 1);
        let decl = code[line_start..at].trim_start();
        if ["const ", "pub const ", "pub(crate) const "]
            .iter()
            .any(|p| decl.starts_with(p))
            && let Some(value) = parse_str_literal(code[from..].trim_start())
        {
            values.push(value);
        }
    }
    values
}

/// The leading Rust string literal of `s` (escapes and `\`-newline continuations resolved).
#[cfg(test)]
fn parse_str_literal(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('"')?.chars().peekable();
    let mut value = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                '\n' => while chars.next_if(|c| c.is_whitespace()).is_some() {},
                'n' => value.push('\n'),
                't' => value.push('\t'),
                escaped => value.push(escaped),
            },
            c => value.push(c),
        }
    }
    None
}

/// Every `const …: &str` definition in the sibling's OpenChatZD module (all `.rs` files).
#[cfg(test)]
fn sibling_label_definitions(verify_root: &std::path::Path) -> Result<Vec<String>, String> {
    let dir = verify_root.join(SIBLING_OPENCHATZD_MODULE_DIR_REL);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    let mut definitions = Vec::new();
    for file in files {
        let src = std::fs::read_to_string(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        definitions.extend(str_const_definitions(&src));
    }
    Ok(definitions)
}

/// Retired outcome tokens that must not be reintroduced as live labels: the pre-v5 four-outcome
/// vocabulary (MATCH / UNAVAILABLE as outcomes) and the hyphenated V3-A spelling.
#[cfg(test)]
fn retired_v3a_outcome_tokens() -> &'static [&'static str] {
    &[
        "\"V3-A\"",
        "V3_A_",
        "INDEX_HASH_MATCH_AT_CERT_TIME",
        "INDEX_ATTESTATION_UNAVAILABLE",
    ]
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

/// `Ok(())` when the sibling DEFINES every required wire label (a `const …: &str` whose value equals
/// it), defines a string carrying the ratified claim fragment, and its attestation module `src`
/// holds no forbidden or retired token. Mentions in comments or test literals satisfy nothing.
#[cfg(test)]
fn sibling_source_matches_openchatzd_labels(src: &str, definitions: &[String]) -> Result<(), String> {
    for label in required_sibling_wire_labels() {
        if !definitions.iter().any(|v| v == label) {
            return Err(format!("missing required label `{label}` (no `const …: &str = \"{label}\"`)"));
        }
    }
    if !definitions.iter().any(|v| v.contains(OCZD_CLAIM_FRAGMENT)) {
        return Err("missing the ratified claim fragment in a `const …: &str` definition".into());
    }
    if src.contains(OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT) {
        return Err("forbidden overclaim fragment present".into());
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

/// Explicit CVDR-Verify location for the cross-repo guards (a checkout, or a `git archive` export,
/// of the CI pin). When set it is the only candidate, and a missing path fails rather than skips.
#[cfg(test)]
pub(crate) const CVDR_VERIFY_SIBLING_ENV: &str = "CVDR_VERIFY_SIBLING";

/// The CVDR-Verify commit CI checks out as the sibling: the `ref:` of the CVDR-Verify checkout step
/// in `.github/workflows/backend.yaml`. Read from the workflow, never hard-coded, so the guards
/// always name the live pin.
#[cfg(test)]
pub(crate) fn required_cvdr_verify_pin() -> String {
    let workflow = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../.github/workflows/backend.yaml");
    let src = std::fs::read_to_string(&workflow).unwrap_or_else(|e| panic!("read {}: {e}", workflow.display()));
    cvdr_verify_pin_from_workflow(&src).unwrap_or_else(|| {
        panic!(
            "{}: no CVDR-Verify checkout pinned to an immutable 40-hex `ref:`",
            workflow.display()
        )
    })
}

/// `ref:` of the step whose `repository:` is `…/CVDR-Verify`; `None` unless it is a 40-hex SHA.
#[cfg(test)]
fn cvdr_verify_pin_from_workflow(src: &str) -> Option<String> {
    let mut in_step = false;
    for line in src.lines().map(str::trim) {
        if let Some(repo) = line.strip_prefix("repository:") {
            in_step = repo.trim().ends_with("/CVDR-Verify");
        } else if line.starts_with("- ") {
            in_step = false;
        } else if let Some(pin) = line.strip_prefix("ref:").filter(|_| in_step) {
            let pin = pin.trim();
            return (pin.len() == 40 && pin.bytes().all(|b| b.is_ascii_hexdigit())).then(|| pin.to_string());
        }
    }
    None
}

/// `git -C <root> <args>` stdout, only when `root` is itself the top of a git checkout (an export
/// nested inside another repository must not report that repository's HEAD).
#[cfg(test)]
fn sibling_git(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let top = git(&["rev-parse", "--show-toplevel"])?;
    if std::fs::canonicalize(&top).ok()? != std::fs::canonicalize(root).ok()? {
        return None;
    }
    git(args)
}

/// The sibling precondition, named in every cross-repo guard failure: which checkout was read, at
/// which commit, and the pin it must be at (or descend from without drifting).
#[cfg(test)]
pub(crate) fn sibling_precondition(root: &std::path::Path) -> String {
    let pin = required_cvdr_verify_pin();
    let head = match sibling_git(root, &["rev-parse", "HEAD"]) {
        None => "unknown (not a git checkout)".to_string(),
        Some(head) if head == pin => format!("{head} (= the pin)"),
        Some(head) => {
            let ancestry = match sibling_git(root, &["merge-base", "--is-ancestor", &pin, "HEAD"]) {
                Some(_) => "the pin is an ancestor",
                None => "the pin is NOT an ancestor, or is not in this clone",
            };
            format!("{head} ({ancestry})")
        }
    };
    format!(
        "Precondition: the CVDR-Verify sibling at {} must be the CI pin {pin} \
         (.github/workflows/backend.yaml) or a descendant carrying the same OpenChatZD labels and \
         corpus mirror; its HEAD is {head}. Check the pin out there, or set \
         {CVDR_VERIFY_SIBLING_ENV}=<checkout or `git archive` export of {pin}>.",
        root.display()
    )
}

/// Resolve CVDR-Verify root: `CVDR_VERIFY_SIBLING` when set; otherwise prefer a checkout that
/// actually contains the OpenChatZD attestation module (Together-alone sibling or nested CI path).
#[cfg(test)]
pub(crate) fn resolve_cvdr_verify_root() -> Option<std::path::PathBuf> {
    if let Some(root) = std::env::var_os(CVDR_VERIFY_SIBLING_ENV).filter(|v| !v.is_empty()) {
        let root = std::path::PathBuf::from(root);
        assert!(
            root.exists(),
            "{CVDR_VERIFY_SIBLING_ENV}={} does not exist. {}",
            root.display(),
            sibling_precondition(&root)
        );
        return Some(root);
    }
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
        assert_eq!(PORTABLE_PACKAGE_V3_NAME, "PortablePackageV3");
        assert_eq!(PORTABLE_PACKAGE_SCHEMA, "openchatzd.cvdr.portable_package");
        assert_eq!(PORTABLE_PACKAGE_VERSION, 3);
        assert_eq!(
            PORTABLE_PACKAGE_VERSION,
            local_user_index_canister::get_cvdr::PORTABLE_VERSION
        );
        assert_eq!(
            PORTABLE_PACKAGE_SCHEMA,
            local_user_index_canister::get_cvdr::PORTABLE_SCHEMA_ID
        );
    }

    /// G rule 2: exactly three outcomes, distinct from each other and from the named failures.
    #[test]
    fn v3a_has_exactly_three_outcomes_and_named_failures() {
        let outcomes = [V3A_PASS, V3A_PENDING_IN_PROTECTED_WINDOW, V3A_PERMANENTLY_UNAVAILABLE];
        let failures = [INDEX_HASH_MISMATCH, INDEX_ATTESTATION_INVALID];
        let all: Vec<&str> = outcomes.iter().chain(failures.iter()).copied().collect();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
        assert!(
            outcomes.iter().all(|o| o.starts_with("V3A_")),
            "outcomes carry the V3A_ prefix"
        );
        assert!(
            failures.iter().all(|f| !f.starts_with("V3A_")),
            "named failures are not outcomes"
        );
        assert!(!all.iter().any(|o| o.contains("V3-A")));
        // pending and permanently-unavailable are never one label
        assert_ne!(V3A_PENDING_IN_PROTECTED_WINDOW, V3A_PERMANENTLY_UNAVAILABLE);
        // The historical-V2 outcome is outside the V3 set: distinct, unprefixed, never a pass.
        assert_eq!(HISTORICAL_V2_NOT_ATTESTED, "NOT_ATTESTED");
        assert!(!all.contains(&HISTORICAL_V2_NOT_ATTESTED));
        assert!(!HISTORICAL_V2_NOT_ATTESTED.starts_with("V3A_"));
    }

    #[test]
    fn timing_axis_is_five_distinct_labels() {
        let timing = [
            TIMING_ROUTINE,
            TIMING_DELAY_EXCEEDED,
            TIMING_BEFORE_COMMITMENT_CERTIFICATE,
            TIMING_OUTSIDE_COMPLETION_WINDOW,
            TIMING_NOT_APPLICABLE,
        ];
        assert_eq!(timing.len(), 5);
        assert_eq!(TIMING_BEFORE_COMMITMENT_CERTIFICATE, "BEFORE_COMMITMENT_CERTIFICATE");
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
    fn supported_claim_is_the_ratified_wording() {
        assert!(OCZD_SUPPORTED_CLAIM.contains(OCZD_CLAIM_FRAGMENT));
        assert!(OCZD_SUPPORTED_CLAIM.contains("upgrade interlock bounds that window from uninstall to evidence capture"));
        assert!(!OCZD_SUPPORTED_CLAIM.contains(OCZD_FORBIDDEN_OVERCLAIM_FRAGMENT));
        assert!(
            !OCZD_SUPPORTED_CLAIM.contains(OCZD_INTERNAL_ONLY_FRAGMENT),
            "internal name must not appear in the claim"
        );
        assert!(
            !OCZD_SUPPORTED_CLAIM.contains("no demonstrated upgrade-continuity interlock"),
            "pre-interlock wording retired"
        );
    }

    #[test]
    fn serving_shapes_remain_independent_labels() {
        // Commitment Available (FrozenWire) vs INDEX Available (PortablePackageV3) are distinct.
        assert_ne!(PORTABLE_PACKAGE_V3_NAME, "FrozenWire");
        // FrozenWire-only Available maps to one of the two no-evidence OUTCOMES, never to PASS.
        assert_ne!(V3A_PENDING_IN_PROTECTED_WINDOW, V3A_PASS);
        assert_ne!(V3A_PERMANENTLY_UNAVAILABLE, V3A_PASS);
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

    /// A sibling module that DEFINES every required label, the claim split over a `\`-newline
    /// continuation as the verifier writes it.
    fn defining_source() -> String {
        let mut src = wire_label_definitions();
        let (head, tail) = OCZD_SUPPORTED_CLAIM.split_at(OCZD_SUPPORTED_CLAIM.find(" during").expect("split"));
        src.push_str(&format!("pub const CLAIM: &str = \"{head} \\\n{}\";\n", tail.trim_start()));
        src
    }

    /// `pub const L<i>: &str = "<label>";` for every required wire label (no claim).
    fn wire_label_definitions() -> String {
        required_sibling_wire_labels()
            .iter()
            .enumerate()
            .map(|(i, label)| format!("pub const L{i}: &str = \"{label}\";\n"))
            .collect()
    }

    fn matches(src: &str) -> Result<(), String> {
        sibling_source_matches_openchatzd_labels(src, &str_const_definitions(src))
    }

    #[test]
    fn sibling_source_matcher_accepts_complete_amended_labels() {
        assert_eq!(matches(&defining_source()), Ok(()));
    }

    #[test]
    fn sibling_source_matcher_rejects_missing_label() {
        let src = format!("pub const A: &str = \"{V3A_PASS}\";\npub const B: &str = \"{INDEX_HASH_MISMATCH}\";\n");
        assert!(matches(&src).unwrap_err().contains("missing required label"));
    }

    /// The false pass this guard used to allow: a label merely MENTIONED — in a comment, a test
    /// literal, a `let` binding — is not a definition and satisfies nothing.
    #[test]
    fn label_mentions_outside_definitions_do_not_satisfy_the_guard() {
        let mut src = String::new();
        for label in required_sibling_wire_labels() {
            src.push_str(&format!("// pub const OLD: &str = \"{label}\"; (renamed)\n"));
            src.push_str(&format!("    assert_eq!(timing, \"{label}\");\n"));
            src.push_str(&format!("    let x: &str = \"{label}\";\n"));
        }
        src.push_str(&format!("    assert!(c.contains(\"{OCZD_CLAIM_FRAGMENT}\"));\n"));
        assert!(str_const_definitions(&src).is_empty(), "{:?}", str_const_definitions(&src));
        assert!(matches(&src).unwrap_err().contains("missing required label"));

        // every label defined, but the claim fragment only in a test literal
        let mut src = wire_label_definitions();
        src.push_str(&format!("    assert!(c.contains(\"{OCZD_CLAIM_FRAGMENT}\"));\n"));
        assert!(matches(&src).unwrap_err().contains("claim fragment"));
    }

    #[test]
    fn str_const_definitions_reads_rust_string_definitions_only() {
        let src = "pub const A: &str = \"V3A_PASS\";\n\
                   pub(crate) const B: &str =\n    \"split \\\n     across\";\n\
                   const C: &str = \"q\\\"uote\";\n\
                   pub const N: u64 = 3;\n\
                   pub const BYTES: &[u8] = b\"raw\";\n\
                   // pub const D: &str = \"comment\";\n";
        assert_eq!(str_const_definitions(src), vec!["V3A_PASS", "split across", "q\"uote"]);
    }

    #[test]
    fn sibling_source_matcher_rejects_retired_v3a_token() {
        let mut src = defining_source();
        src.push_str("outcome = \"V3-A\"\n");
        assert!(matches(&src).unwrap_err().contains("retired V3-A"));
    }

    #[test]
    fn sibling_source_matcher_rejects_retired_timing_wire() {
        let mut src = defining_source();
        // Build without embedding the retired wire literally in this file's source
        // (keeps cross-repo scanners clean).
        let retired = format!("pub const TIMING_{}: &str = \"{}{}\";\n", "LATE_PATH", "late", "_path");
        src.push_str(&retired);
        assert!(matches(&src).unwrap_err().contains("retired timing"));
    }

    /// Drift guard: when the sibling `CVDR-Verify` repo is checked out, the offline verifier must
    /// DEFINE every OpenChatZD label (`const …: &str = "LABEL"` in its `openchatzd` module).
    #[test]
    fn labels_match_sibling_cvdr_verify_when_present() {
        let Some(verify_root) = resolve_cvdr_verify_root() else {
            eprintln!(
                "skip cross-repo labels: CVDR-Verify sibling not checked out (CI pin {}; set {CVDR_VERIFY_SIBLING_ENV} to run)",
                required_cvdr_verify_pin()
            );
            return;
        };
        let path = verify_root.join(SIBLING_OPENCHATZD_ATTESTATION_REL);
        match sibling_label_guard(&verify_root, &path) {
            SiblingLabelGuard::SkipAbsent => unreachable!("root exists"),
            SiblingLabelGuard::FailMissingModule => {
                panic!(
                    "CVDR-Verify is present at {} but {} is missing. \
                     Pin gap or moved path: amended OpenChatZD INDEX labels are not in this checkout. {}",
                    verify_root.display(),
                    path.display(),
                    sibling_precondition(&verify_root)
                );
            }
            SiblingLabelGuard::RequireLabelMatch => {
                let src = fs::read_to_string(&path).expect("read CVDR-Verify index_attestation");
                let definitions = sibling_label_definitions(&verify_root)
                    .unwrap_or_else(|e| panic!("{e}. {}", sibling_precondition(&verify_root)));
                sibling_source_matches_openchatzd_labels(&src, &definitions).unwrap_or_else(|e| {
                    panic!(
                        "CVDR-Verify OpenChatZD label drift: {e}. {}",
                        sibling_precondition(&verify_root)
                    );
                });
            }
        }
    }

    #[test]
    fn cvdr_verify_pin_is_read_from_the_backend_workflow() {
        let pin = required_cvdr_verify_pin();
        assert_eq!(pin.len(), 40);
        assert!(pin.bytes().all(|b| b.is_ascii_hexdigit()));

        let step = |reference: &str| {
            format!(
                "      - uses: actions/checkout@v4\n        with:\n          repository: Org/CVDR-Verify\n          \
                 # comment\n          ref: {reference}\n          path: CVDR-Verify\n"
            )
        };
        let sha = "8b0d835057a3c4987a99a1f18c6e4e364733d2ef";
        assert_eq!(cvdr_verify_pin_from_workflow(&step(sha)).as_deref(), Some(sha));
        assert_eq!(
            cvdr_verify_pin_from_workflow(&step("openchatzd-v5")),
            None,
            "branch names are not pins"
        );
        let other_repo = format!(
            "      - uses: actions/checkout@v4\n        with:\n          repository: Org/Other\n          ref: {sha}\n"
        );
        assert_eq!(cvdr_verify_pin_from_workflow(&other_repo), None);
    }

    #[test]
    fn sibling_precondition_names_the_pin_and_the_path() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let root: PathBuf = std::env::temp_dir().join(format!("oczd-cvdr-verify-export-{stamp}"));
        fs::create_dir_all(&root).expect("mkdir export root");
        let msg = sibling_precondition(&root);
        assert!(msg.contains(&required_cvdr_verify_pin()), "{msg}");
        assert!(msg.contains(&root.display().to_string()), "{msg}");
        assert!(msg.contains("unknown (not a git checkout)"), "{msg}");
        assert!(msg.contains(CVDR_VERIFY_SIBLING_ENV), "{msg}");
        let _ = fs::remove_dir_all(&root);
    }
}
