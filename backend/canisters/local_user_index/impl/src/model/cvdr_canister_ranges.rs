//! Independent fail-closed canister-range authorization for the CVDR store-gate.
//!
//! The upstream `ic-certificate-verification` crate can vacuous-pass when sharded
//! `/canister_ranges` leaves do not resolve to `Found` (or when the resolved set is
//! empty). Stef ruling: assert containment independently after `cert.verify`, covering
//! both layouts; malformed / unresolved shards and an empty resolved set fail closed.
//! Semantics aligned with CVDR-Verify `authorize_canister_ranges` (no dependency —
//! that crate is host-side).

use candid::Principal;
use ic_certification::{Certificate, LookupResult, SubtreeLookupResult};

/// Distinct failure modes for the independent range check (D2 honest taxonomy).
/// Mapped into [`crate::model::cvdr::CertRejectReason`] at the store-gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeAuthError {
    /// Ranges present and decoded, but `effective_canister_id` is outside every range.
    NotInRange,
    /// Neither legacy nor sharded layout yields a non-empty resolved range set.
    RangesMissing,
    /// Delegation CBOR / nested delegation / shard depth / leaf CBOR decode failure.
    Malformed,
}

impl RangeAuthError {
    pub fn as_str(self) -> &'static str {
        match self {
            RangeAuthError::NotInRange => "certificate_canister_not_in_range",
            RangeAuthError::RangesMissing => "certificate_canister_ranges_missing",
            RangeAuthError::Malformed => "certificate_canister_ranges_malformed",
        }
    }
}

/// Authorize `effective_canister_id` against ranges proven in `delegated_cert` for `subnet_id`.
///
/// - **Legacy:** `/subnet/<subnet_id>/canister_ranges` — single CBOR `Vec<(Principal, Principal)>`.
/// - **Sharded:** `/canister_ranges/<subnet_id>/<shard_key>` — every present leaf decoded;
///   non-`Found`, wrong depth, or decode failure rejects (no silent skip); empty present set
///   after a Found subtree is treated as missing (no vacuous pass).
pub fn authorize_canister_ranges(
    delegated_cert: &Certificate,
    subnet_id: &[u8],
    effective_canister_id: &Principal,
) -> Result<(), RangeAuthError> {
    // Legacy single-blob layout.
    if let LookupResult::Found(blob) = delegated_cert.tree.lookup_path([
        b"subnet".as_ref(),
        subnet_id,
        b"canister_ranges".as_ref(),
    ]) {
        let ranges: Vec<(Principal, Principal)> = serde_cbor::from_slice(blob)
            .map_err(|_| RangeAuthError::Malformed)?;
        return if principal_is_within_ranges(effective_canister_id, &ranges) {
            Ok(())
        } else {
            Err(RangeAuthError::NotInRange)
        };
    }

    // Sharded layout.
    if let SubtreeLookupResult::Found(subtree) = delegated_cert
        .tree
        .lookup_subtree([b"canister_ranges".as_ref(), subnet_id])
    {
        const SHARDED_LEAF_DEPTH: usize = 1;

        let mut present_shard = false;
        let mut authorized = false;
        for path in subtree.list_paths() {
            present_shard = true;
            if path.len() != SHARDED_LEAF_DEPTH {
                return Err(RangeAuthError::Malformed);
            }
            let leaf = match subtree.lookup_path(&path) {
                LookupResult::Found(leaf) => leaf,
                _ => return Err(RangeAuthError::Malformed),
            };
            let ranges: Vec<(Principal, Principal)> =
                serde_cbor::from_slice(leaf).map_err(|_| RangeAuthError::Malformed)?;
            if principal_is_within_ranges(effective_canister_id, &ranges) {
                authorized = true;
            }
        }
        if present_shard {
            return if authorized {
                Ok(())
            } else {
                Err(RangeAuthError::NotInRange)
            };
        }
        // Found subtree but no present leaves → empty resolved set → fail closed below.
    }

    Err(RangeAuthError::RangesMissing)
}

/// After BLS/`cert.verify`, independently re-check range containment on the delegation
/// certificate. No delegation → nothing extra (root-signed path already checked by verify).
pub fn assert_delegation_range_containment(
    cert: &Certificate,
    effective_canister_id: Principal,
) -> Result<(), RangeAuthError> {
    let Some(delegation) = &cert.delegation else {
        return Ok(());
    };
    let delegated: Certificate = serde_cbor::from_slice(delegation.certificate.as_ref())
        .map_err(|_| RangeAuthError::Malformed)?;
    if delegated.delegation.is_some() {
        return Err(RangeAuthError::Malformed);
    }
    authorize_canister_ranges(
        &delegated,
        delegation.subnet_id.as_ref(),
        &effective_canister_id,
    )
}

fn principal_is_within_ranges(principal: &Principal, ranges: &[(Principal, Principal)]) -> bool {
    let p = principal.as_slice();
    ranges
        .iter()
        .any(|(lo, hi)| p >= lo.as_slice() && p <= hi.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_certification::{fork, label, leaf, HashTree};

    const SUBNET_ID: &[u8] = &[0xfe, 0x32, 0x0f, 0x2f, 0xbb];
    const OTHER_SUBNET_ID: &[u8] = &[0xab, 0xcd, 0xef, 0x01, 0x23];
    const RANGE_LOW: &[u8] = &[0, 0, 0, 0, 1, 0x30, 0, 0, 1, 1];
    const RANGE_HIGH: &[u8] = &[0, 0, 0, 0, 1, 0x3f, 0xff, 0xff, 1, 1];
    const IN_RANGE: &[u8] = &[0, 0, 0, 0, 1, 0x30, 0x90, 0x42, 1, 1];
    const OUT_OF_RANGE: &[u8] = &[0, 0, 0, 0, 1, 0x40, 0, 0, 1, 1];

    fn ranges_cbor() -> Vec<u8> {
        let ranges: Vec<(Principal, Principal)> = vec![(
            Principal::from_slice(RANGE_LOW),
            Principal::from_slice(RANGE_HIGH),
        )];
        serde_cbor::to_vec(&ranges).unwrap()
    }

    fn cert_with_tree(tree: HashTree) -> Certificate {
        Certificate {
            tree,
            signature: Vec::new(),
            delegation: None,
        }
    }

    fn legacy_cert() -> Certificate {
        cert_with_tree(label(
            "subnet",
            label(SUBNET_ID, label("canister_ranges", leaf(ranges_cbor()))),
        ))
    }

    fn sharded_cert() -> Certificate {
        cert_with_tree(label(
            "canister_ranges",
            label(SUBNET_ID, label(RANGE_LOW, leaf(ranges_cbor()))),
        ))
    }

    fn sharded_cert_malformed_leaf() -> Certificate {
        cert_with_tree(label(
            "canister_ranges",
            label(SUBNET_ID, label(RANGE_LOW, leaf(vec![0xff, 0xff, 0xff]))),
        ))
    }

    fn sharded_cert_wrong_subnet() -> Certificate {
        cert_with_tree(label(
            "canister_ranges",
            label(OTHER_SUBNET_ID, label(RANGE_LOW, leaf(ranges_cbor()))),
        ))
    }

    fn sharded_cert_valid_then_malformed() -> Certificate {
        cert_with_tree(label(
            "canister_ranges",
            label(
                SUBNET_ID,
                fork(
                    label(RANGE_LOW, leaf(ranges_cbor())),
                    label(RANGE_HIGH, leaf(vec![0xff, 0xff, 0xff])),
                ),
            ),
        ))
    }

    fn sharded_cert_deeper_path() -> Certificate {
        cert_with_tree(label(
            "canister_ranges",
            label(
                SUBNET_ID,
                label(RANGE_LOW, label("extra", leaf(ranges_cbor()))),
            ),
        ))
    }

    fn empty_sharded_subtree() -> Certificate {
        // Found /canister_ranges/<subnet> with no leaf children — empty resolved set.
        cert_with_tree(label(
            "canister_ranges",
            label(SUBNET_ID, ic_certification::empty()),
        ))
    }

    #[test]
    fn legacy_layout_authorizes_in_range() {
        authorize_canister_ranges(&legacy_cert(), SUBNET_ID, &Principal::from_slice(IN_RANGE))
            .expect("in-range canister must be authorized via legacy layout");
    }

    #[test]
    fn sharded_layout_authorizes_in_range() {
        authorize_canister_ranges(&sharded_cert(), SUBNET_ID, &Principal::from_slice(IN_RANGE))
            .expect("in-range canister must be authorized via sharded layout");
    }

    #[test]
    fn legacy_layout_rejects_out_of_range() {
        assert_eq!(
            authorize_canister_ranges(
                &legacy_cert(),
                SUBNET_ID,
                &Principal::from_slice(OUT_OF_RANGE),
            ),
            Err(RangeAuthError::NotInRange)
        );
    }

    #[test]
    fn sharded_layout_rejects_out_of_range() {
        assert_eq!(
            authorize_canister_ranges(
                &sharded_cert(),
                SUBNET_ID,
                &Principal::from_slice(OUT_OF_RANGE),
            ),
            Err(RangeAuthError::NotInRange)
        );
    }

    #[test]
    fn rejects_when_ranges_absent_from_both_layouts() {
        let cert = cert_with_tree(label("time", leaf(vec![1, 2, 3])));
        assert_eq!(
            authorize_canister_ranges(&cert, SUBNET_ID, &Principal::from_slice(IN_RANGE)),
            Err(RangeAuthError::RangesMissing)
        );
    }

    #[test]
    fn empty_resolved_sharded_set_is_authorization_failure() {
        assert_eq!(
            authorize_canister_ranges(
                &empty_sharded_subtree(),
                SUBNET_ID,
                &Principal::from_slice(IN_RANGE),
            ),
            Err(RangeAuthError::RangesMissing)
        );
    }

    #[test]
    fn sharded_layout_rejects_malformed_cbor_leaf() {
        assert_eq!(
            authorize_canister_ranges(
                &sharded_cert_malformed_leaf(),
                SUBNET_ID,
                &Principal::from_slice(IN_RANGE),
            ),
            Err(RangeAuthError::Malformed)
        );
    }

    #[test]
    fn rejects_when_ranges_signed_under_wrong_subnet() {
        assert_eq!(
            authorize_canister_ranges(
                &sharded_cert_wrong_subnet(),
                SUBNET_ID,
                &Principal::from_slice(IN_RANGE),
            ),
            Err(RangeAuthError::RangesMissing)
        );
    }

    #[test]
    fn sharded_layout_rejects_when_a_later_leaf_is_malformed() {
        assert_eq!(
            authorize_canister_ranges(
                &sharded_cert_valid_then_malformed(),
                SUBNET_ID,
                &Principal::from_slice(IN_RANGE),
            ),
            Err(RangeAuthError::Malformed)
        );
    }

    #[test]
    fn sharded_layout_rejects_deeper_descendant_path() {
        assert_eq!(
            authorize_canister_ranges(
                &sharded_cert_deeper_path(),
                SUBNET_ID,
                &Principal::from_slice(IN_RANGE),
            ),
            Err(RangeAuthError::Malformed)
        );
    }

    #[test]
    fn assert_delegation_ok_when_no_delegation() {
        assert_eq!(
            assert_delegation_range_containment(&legacy_cert(), Principal::from_slice(IN_RANGE)),
            Ok(())
        );
    }

    #[test]
    fn range_auth_error_strings_are_distinct() {
        assert_ne!(RangeAuthError::NotInRange.as_str(), RangeAuthError::RangesMissing.as_str());
        assert_ne!(RangeAuthError::NotInRange.as_str(), RangeAuthError::Malformed.as_str());
        assert_ne!(RangeAuthError::RangesMissing.as_str(), RangeAuthError::Malformed.as_str());
    }
}
