//! # MKTd02 (ICP-Delete-Leaf) Leaf-mode CVDR integration — P1
//!
//! This module wires the OpenChatZD **user** canister to the MKTd02 deletion
//! engine (`mktd02` @ `mktd02-v0.4.0`) as a single-subject **Leaf** integration.
//!
//! It provides:
//! - [`MKTdUserAdapter`] — the [`mktd02::MKTdDataSource`] implementation mapping
//!   the user canister's PII (heap `Data` fields + the direct-chat message
//!   bodies in the stable map) to the engine's deterministic state model.
//! - [`record_id_for`] — the host-supplied `record_id` derivation (D7 / S5).
//! - [`config`] — the engine config fixing `base_memory_id = 100` (S6 / O6).
//!
//! ## Re-entrancy contract (IMPORTANT)
//!
//! The engine calls `adapter.get_state_bytes()` / `tombstone_state()` /
//! `is_tombstoned()` back into this module, and each of those acquires the
//! canister state via `read_state` / `mutate_state`. Therefore callers that
//! invoke the engine (the Phase A wrapper) MUST NOT already hold a `read_state`
//! / `mutate_state` borrow — i.e. do **not** call the engine from inside
//! `execute_update`. See `updates/mktd_execute_deletion.rs`.
//!
//! ## Cross-region snapshot / tombstone (W2)
//!
//! PII spans two physical regions: heap `Data` (profile/scalar PII, only
//! serialized to MemoryId 0 at upgrade) and the unified `stable_memory_map`
//! (MemoryId 3, holding direct-chat message bodies). The adapter:
//! - `get_state_bytes()` derives a deterministic snapshot from the **live heap
//!   `Data`** (which includes the direct-chat *structure*), guaranteeing that
//!   the post-tombstone snapshot differs within the SAME message — it does NOT
//!   depend on the asynchronous stable-map purge completing.
//! - `tombstone_state()` clears the heap PII fields synchronously AND enqueues
//!   the existing deferred stable-map purge (`stable_memory_keys_to_garbage_collect`)
//!   for the message bodies, reusing [`crate::RuntimeState::delete_direct_chat`]
//!   (heap removal is synchronous; byte reclamation is deferred). No
//!   inter-canister calls are made (single-message atomicity — D8 constraint).
//!
//! See `docs/State_Encoding_Spec` (in the P1 deliverable) for the exact preimage.

use crate::{RuntimeState, mutate_state_bypass, read_state};
use ic_principal::Principal;
use mktd02::{CommitMode, MKTdDataSource, MktdConfig};
use serde::{Deserialize, Serialize};
use types::{Achievement, UserId};
use zombie_core::FieldDescriptor;
use zombie_core::hashing::{DomainTag, hash_with_tag};
use zombie_core::serialisation::encode_pii_state;
use zombie_core::tombstone::tombstone_constant;

/// MKTd02's base MemoryId. The engine reserves 8 slots: 100..=107
/// (meta, state_hash, deletion_seq, certified_commitment, deletion_event_hash,
/// finalization_lock, receipts, tombstoned_at). The host uses only {0, 3}, so
/// 100..=107 is collision-free with the entire 4..=99 band as buffer (S6/O6).
pub const BASE_MEMORY_ID: u8 = 100;

/// Domain tag for the OpenChatZD user `record_id` derivation (S5 ADR).
/// `record_id = SHA-256( "OPENCHATZD_RECORD_ID_USER_V1" || canonical_user_id_bytes )`.
const RECORD_ID_TAG: DomainTag = DomainTag(b"OPENCHATZD_RECORD_ID_USER_V1");

/// Version identifier for the PII state encoding (State Encoding Spec v2).
/// Bump this (and the spec) for ANY change to the preimage below.
pub const PII_ENCODER_VERSION: &str = "OPENCHATZD_USER_PII_V2";

/// Engine configuration for this canister.
pub fn config() -> MktdConfig {
    MktdConfig {
        base_memory_id: BASE_MEMORY_ID,
    }
}

/// Derive the host-supplied, domain-tagged `record_id` (D7 / S5).
///
/// Input: the canonical durable OpenChat **UserId** — the user canister's own
/// principal (`UserId(CanisterId)`), which is permanent for the life of the
/// account. Encoded as the principal's raw bytes (`Principal::as_slice`), the
/// IC's canonical principal representation.
///
/// - **Never `caller()`** — `caller()` is the owner's *identity* principal,
///   which can change via identity-linking; the UserId is the stable subject.
/// - The returned value is the **32-byte SHA-256 hash**, not a raw identifier,
///   so the receipt never stores a raw principal as `record_id`.
///
/// The engine treats `record_id` as opaque bytes and feeds it verbatim into
/// `compute_receipt_id` (it binds this receipt's `receipt_id`, but NOT the
/// certified commitment — see S5 trust boundary).
pub fn record_id_for(user_id: UserId) -> Vec<u8> {
    let principal: Principal = user_id.into();
    hash_with_tag(RECORD_ID_TAG, &[principal.as_slice()]).to_vec()
}

// ---------------------------------------------------------------------------
// Deterministic PII projection (State Encoding Spec v1)
// ---------------------------------------------------------------------------
//
// Rules (engine docs/sections/11): struct fields in manifest order, no maps or
// floats, encoded only through `encode_pii_state`. Collections are projected
// to deterministic vectors containing their complete in-scope contents.

#[derive(Serialize, Deserialize)]
enum PiiValue<T> {
    Active(T),
    Tombstone([u8; 32]),
}

impl<T> PiiValue<T> {
    fn tombstone() -> Self {
        Self::Tombstone(*tombstone_constant())
    }

    fn is_canonical_tombstone(&self) -> bool {
        matches!(self, Self::Tombstone(value) if value == tombstone_constant())
    }
}

#[derive(Serialize, Deserialize)]
struct PiiState {
    encoder_version: String,
    bio: PiiValue<String>,
    username: PiiValue<String>,
    display_name: PiiValue<Option<String>>,
    avatar: PiiValue<Option<types::Document>>,
    profile_background: PiiValue<Option<types::Document>>,
    unique_person_proof: PiiValue<Option<types::UniquePersonProof>>,
    contacts: PiiValue<Vec<(Vec<u8>, Option<String>)>>,
    blocked_users: PiiValue<Vec<Vec<u8>>>,
    achievements: PiiValue<Vec<u16>>,
    external_achievements: PiiValue<Vec<String>>,
    message_activity_events: PiiValue<(Vec<user_canister::MessageActivityEvent>, u64, u64)>,
    phone_is_verified: PiiValue<bool>,
    referred_by: PiiValue<Option<Vec<u8>>>,
}

impl PiiState {
    fn tombstoned() -> PiiState {
        PiiState {
            encoder_version: PII_ENCODER_VERSION.to_string(),
            bio: PiiValue::tombstone(),
            username: PiiValue::tombstone(),
            display_name: PiiValue::tombstone(),
            avatar: PiiValue::tombstone(),
            profile_background: PiiValue::tombstone(),
            unique_person_proof: PiiValue::tombstone(),
            contacts: PiiValue::tombstone(),
            blocked_users: PiiValue::tombstone(),
            achievements: PiiValue::tombstone(),
            external_achievements: PiiValue::tombstone(),
            message_activity_events: PiiValue::tombstone(),
            phone_is_verified: PiiValue::tombstone(),
            referred_by: PiiValue::tombstone(),
        }
    }

    fn snapshot(state: &RuntimeState) -> PiiState {
        let d = &state.data;

        if d.pii_tombstoned {
            return PiiState::tombstoned();
        }

        let mut contacts: Vec<(Vec<u8>, Option<String>)> = d
            .contacts
            .iter()
            .map(|(user_id, contact)| (Principal::from(*user_id).as_slice().to_vec(), contact.nickname.clone()))
            .collect();
        contacts.sort();

        let mut blocked: Vec<Vec<u8>> = d
            .blocked_users
            .value
            .iter()
            .map(|u| Principal::from(*u).as_slice().to_vec())
            .collect();
        blocked.sort();

        let mut achievements: Vec<u16> = d.achievements.iter().map(achievement_id).collect();
        achievements.sort_unstable();

        let mut external_achievements: Vec<String> = d.external_achievements.iter().cloned().collect();
        external_achievements.sort();

        PiiState {
            encoder_version: PII_ENCODER_VERSION.to_string(),
            bio: PiiValue::Active(d.bio.value.clone()),
            username: PiiValue::Active(d.username.value.clone()),
            display_name: PiiValue::Active(d.display_name.value.clone()),
            avatar: PiiValue::Active(d.avatar.value.clone()),
            profile_background: PiiValue::Active(d.profile_background.value.clone()),
            unique_person_proof: PiiValue::Active(d.unique_person_proof.clone()),
            contacts: PiiValue::Active(contacts),
            blocked_users: PiiValue::Active(blocked),
            achievements: PiiValue::Active(achievements),
            external_achievements: PiiValue::Active(external_achievements),
            message_activity_events: PiiValue::Active(d.message_activity_events.attestation_snapshot()),
            phone_is_verified: PiiValue::Active(d.phone_is_verified),
            referred_by: PiiValue::Active(d.referred_by.map(|u| Principal::from(u).as_slice().to_vec())),
        }
    }

    fn has_canonical_tombstones(&self) -> bool {
        self.bio.is_canonical_tombstone()
            && self.username.is_canonical_tombstone()
            && self.display_name.is_canonical_tombstone()
            && self.avatar.is_canonical_tombstone()
            && self.profile_background.is_canonical_tombstone()
            && self.unique_person_proof.is_canonical_tombstone()
            && self.contacts.is_canonical_tombstone()
            && self.blocked_users.is_canonical_tombstone()
            && self.achievements.is_canonical_tombstone()
            && self.external_achievements.is_canonical_tombstone()
            && self.message_activity_events.is_canonical_tombstone()
            && self.phone_is_verified.is_canonical_tombstone()
            && self.referred_by.is_canonical_tombstone()
    }
}

fn achievement_id(achievement: &Achievement) -> u16 {
    match achievement {
        Achievement::JoinedGroup => 0,
        Achievement::JoinedCommunity => 1,
        Achievement::SentDirectMessage => 2,
        Achievement::ReceivedDirectMessage => 3,
        Achievement::SetAvatar => 4,
        Achievement::SetBio => 5,
        Achievement::SetDisplayName => 6,
        Achievement::UpgradedToDiamond => 7,
        Achievement::UpgradedToGoldDiamond => 8,
        Achievement::Streak3 => 9,
        Achievement::Streak7 => 10,
        Achievement::Streak14 => 11,
        Achievement::Streak30 => 12,
        Achievement::Streak100 => 13,
        Achievement::Streak365 => 14,
        Achievement::SentPoll => 15,
        Achievement::SentText => 16,
        Achievement::SentImage => 17,
        Achievement::SentVideo => 18,
        Achievement::SentAudio => 19,
        Achievement::SentFile => 20,
        Achievement::SentGiphy => 21,
        Achievement::SentPrize => 22,
        Achievement::SentMeme => 23,
        Achievement::SentCrypto => 24,
        Achievement::SentP2PSwapOffer => 25,
        Achievement::StartedCall => 26,
        Achievement::ReactedToMessage => 27,
        Achievement::EditedMessage => 28,
        Achievement::RepliedInThread => 29,
        Achievement::QuoteReplied => 30,
        Achievement::TippedMessage => 31,
        Achievement::DeletedMessage => 32,
        Achievement::ForwardedMessage => 33,
        Achievement::ProvedUniquePersonhood => 34,
        Achievement::ReceivedCrypto => 35,
        Achievement::HadMessageReactedTo => 36,
        Achievement::HadMessageTipped => 37,
        Achievement::VotedOnPoll => 38,
        Achievement::SentReminder => 39,
        Achievement::JoinedCall => 40,
        Achievement::AcceptedP2PSwapOffer => 41,
        Achievement::SetCommunityDisplayName => 42,
        Achievement::Referred1stUser => 43,
        Achievement::Referred3rdUser => 44,
        Achievement::Referred10thUser => 45,
        Achievement::Referred20thUser => 46,
        Achievement::Referred50thUser => 47,
        Achievement::FollowedThread => 48,
        Achievement::FavouritedChat => 49,
        Achievement::SetPin => 50,
        Achievement::SwappedFromWallet => 51,
        Achievement::PinnedChat => 52,
        Achievement::DepositedBtc => 53,
        Achievement::ChangedTheme => 54,
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// Zero-sized adapter; all state access goes through `read_state`/`mutate_state`
/// (mirrors the DaffyDefs reference adapter's thread-local access pattern).
pub struct MKTdUserAdapter;

impl MKTdDataSource for MKTdUserAdapter {
    fn mode(&self) -> CommitMode {
        CommitMode::Leaf
    }

    /// G-ratified profile/identity PII boundary. Every manifest field has one
    /// matching full-content projection in `PiiState`, in the same order.
    fn pii_field_manifest(&self) -> Vec<FieldDescriptor> {
        let fields: &[(&str, &str)] = &[
            ("bio", "String"),
            ("username", "String"),
            ("display_name", "Option<String>"),
            ("avatar", "Option<Document>"),
            ("profile_background", "Option<Document>"),
            ("unique_person_proof", "Option<UniquePersonProof>"),
            ("contacts", "Contacts"),
            ("blocked_users", "HashSet<UserId>"),
            ("achievements", "HashSet<Achievement>"),
            ("external_achievements", "HashSet<String>"),
            ("message_activity_events", "MessageActivityEvents"),
            ("phone_is_verified", "bool"),
            ("referred_by", "Option<UserId>"),
        ];
        fields
            .iter()
            .enumerate()
            .map(|(i, (name, ty))| FieldDescriptor {
                field_name: (*name).into(),
                field_type: (*ty).into(),
                field_order: i as u32,
            })
            .collect()
    }

    fn get_state_bytes(&self) -> Vec<u8> {
        read_state(|state| {
            let pii = PiiState::snapshot(state);
            encode_pii_state(&pii).expect("MKTd02 adapter: PII state encoding failed")
        })
    }

    fn tombstone_state(&mut self) {
        mutate_state_bypass(|state| {
            let now = state.env.now();
            let d = &mut state.data;
            d.bio = types::Timestamped::new(String::new(), now);
            d.username = types::Timestamped::new(String::new(), now);
            d.display_name = types::Timestamped::default();
            d.avatar = types::Timestamped::default();
            d.profile_background = types::Timestamped::default();
            d.pin_number = crate::model::pin_number::PinNumber::default();
            d.unique_person_proof = None;
            d.contacts = crate::model::contacts::Contacts::default();
            d.blocked_users = types::Timestamped::default();
            d.achievements = std::collections::HashSet::new();
            d.external_achievements = std::collections::HashSet::new();
            d.message_activity_events = crate::model::message_activity_events::MessageActivityEvents::default();
            d.phone_is_verified = false;
            d.referred_by = None;
            d.pii_tombstoned = true;
        });
    }

    fn is_tombstoned(&self) -> bool {
        read_state(|state| {
            let d = &state.data;
            PiiState::snapshot(state).has_canonical_tombstones()
                && d.bio.value.is_empty()
                && d.username.value.is_empty()
                && d.display_name.value.is_none()
                && d.avatar.value.is_none()
                && d.profile_background.value.is_none()
                && !d.pin_number.enabled()
                && d.unique_person_proof.is_none()
                && d.contacts.is_empty()
                && d.blocked_users.value.is_empty()
                && d.achievements.is_empty()
                && d.external_achievements.is_empty()
                && d.message_activity_events.len() == 0
                && !d.phone_is_verified
                && d.referred_by.is_none()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zombie_core::hashing::{TAG_SALT, sha256_concat};

    #[derive(Serialize)]
    struct LegacyPiiState {
        bio: PiiValue<String>,
        username: PiiValue<String>,
        display_name: PiiValue<Option<String>>,
        avatar: PiiValue<Option<types::Document>>,
        profile_background: PiiValue<Option<types::Document>>,
        unique_person_proof: PiiValue<Option<types::UniquePersonProof>>,
        contacts: PiiValue<Vec<(Vec<u8>, Option<String>)>>,
        blocked_users: PiiValue<Vec<Vec<u8>>>,
        achievements: PiiValue<Vec<u16>>,
        external_achievements: PiiValue<Vec<String>>,
        message_activity_events: PiiValue<(Vec<user_canister::MessageActivityEvent>, u64, u64)>,
        phone_is_verified: PiiValue<bool>,
        referred_by: PiiValue<Option<Vec<u8>>>,
    }

    fn active_fixture() -> PiiState {
        PiiState {
            encoder_version: PII_ENCODER_VERSION.to_string(),
            bio: PiiValue::Active("fixture bio".to_string()),
            username: PiiValue::Active("fixture_user".to_string()),
            display_name: PiiValue::Active(Some("Fixture User".to_string())),
            avatar: PiiValue::Active(None),
            profile_background: PiiValue::Active(None),
            unique_person_proof: PiiValue::Active(None),
            contacts: PiiValue::Active(vec![(vec![1, 2, 3], Some("Alice".to_string()))]),
            blocked_users: PiiValue::Active(vec![vec![4, 5, 6]]),
            achievements: PiiValue::Active(vec![0, 5, 34]),
            external_achievements: PiiValue::Active(vec!["external-fixture".to_string()]),
            message_activity_events: PiiValue::Active((Vec::new(), 11, 12)),
            phone_is_verified: PiiValue::Active(true),
            referred_by: PiiValue::Active(Some(vec![7, 8, 9])),
        }
    }

    fn legacy_active_fixture() -> LegacyPiiState {
        LegacyPiiState {
            bio: PiiValue::Active("fixture bio".to_string()),
            username: PiiValue::Active("fixture_user".to_string()),
            display_name: PiiValue::Active(Some("Fixture User".to_string())),
            avatar: PiiValue::Active(None),
            profile_background: PiiValue::Active(None),
            unique_person_proof: PiiValue::Active(None),
            contacts: PiiValue::Active(vec![(vec![1, 2, 3], Some("Alice".to_string()))]),
            blocked_users: PiiValue::Active(vec![vec![4, 5, 6]]),
            achievements: PiiValue::Active(vec![0, 5, 34]),
            external_achievements: PiiValue::Active(vec!["external-fixture".to_string()]),
            message_activity_events: PiiValue::Active((Vec::new(), 11, 12)),
            phone_is_verified: PiiValue::Active(true),
            referred_by: PiiValue::Active(Some(vec![7, 8, 9])),
        }
    }

    fn legacy_tombstoned_fixture() -> LegacyPiiState {
        LegacyPiiState {
            bio: PiiValue::tombstone(),
            username: PiiValue::tombstone(),
            display_name: PiiValue::tombstone(),
            avatar: PiiValue::tombstone(),
            profile_background: PiiValue::tombstone(),
            unique_person_proof: PiiValue::tombstone(),
            contacts: PiiValue::tombstone(),
            blocked_users: PiiValue::tombstone(),
            achievements: PiiValue::tombstone(),
            external_achievements: PiiValue::tombstone(),
            message_activity_events: PiiValue::tombstone(),
            phone_is_verified: PiiValue::tombstone(),
            referred_by: PiiValue::tombstone(),
        }
    }

    fn fixture_state_hash<T: Serialize>(state: &T, canister_id: Principal) -> [u8; 32] {
        let state_bytes = encode_pii_state(state).unwrap();
        let salt = hash_with_tag(TAG_SALT, &[canister_id.as_slice()]);
        sha256_concat(&[&salt, &state_bytes])
    }

    fn assert_tombstone<T>(value: PiiValue<T>) {
        assert!(matches!(value, PiiValue::Tombstone(value) if value == *tombstone_constant()));
    }

    #[test]
    fn manifest_matches_the_ratified_13_field_projection() {
        let names: Vec<_> = MKTdUserAdapter
            .pii_field_manifest()
            .into_iter()
            .map(|field| field.field_name)
            .collect();

        assert_eq!(
            names,
            [
                "bio",
                "username",
                "display_name",
                "avatar",
                "profile_background",
                "unique_person_proof",
                "contacts",
                "blocked_users",
                "achievements",
                "external_achievements",
                "message_activity_events",
                "phone_is_verified",
                "referred_by",
            ]
        );
    }

    #[test]
    fn all_13_projection_fields_use_the_canonical_tombstone() {
        let pii = PiiState::tombstoned();
        assert_eq!(pii.encoder_version, PII_ENCODER_VERSION);
        assert!(pii.has_canonical_tombstones());
        assert_tombstone(pii.bio);
        assert_tombstone(pii.username);
        assert_tombstone(pii.display_name);
        assert_tombstone(pii.avatar);
        assert_tombstone(pii.profile_background);
        assert_tombstone(pii.unique_person_proof);
        assert_tombstone(pii.contacts);
        assert_tombstone(pii.blocked_users);
        assert_tombstone(pii.achievements);
        assert_tombstone(pii.external_achievements);
        assert_tombstone(pii.message_activity_events);
        assert_tombstone(pii.phone_is_verified);
        assert_tombstone(pii.referred_by);
    }

    #[test]
    fn encoder_version_is_hash_affecting() {
        let current = PiiState::tombstoned();
        let mut different = PiiState::tombstoned();
        different.encoder_version = "OPENCHATZD_USER_PII_DIFFERENT".to_string();

        assert_ne!(encode_pii_state(&current).unwrap(), encode_pii_state(&different).unwrap());
    }

    #[test]
    fn encoder_version_binding_fixture_v2() {
        let canister_id = Principal::from_slice(&[1, 2, 3, 4]);
        let before_pre = fixture_state_hash(&legacy_active_fixture(), canister_id);
        let before_post = fixture_state_hash(&legacy_tombstoned_fixture(), canister_id);
        let after_pre = fixture_state_hash(&active_fixture(), canister_id);
        let after_post = fixture_state_hash(&PiiState::tombstoned(), canister_id);

        println!("FIXTURE=encoder_version_binding_fixture_v2");
        println!("CANISTER_ID={canister_id}");
        println!(
            "BEFORE_FIELD_ORDER=bio,username,display_name,avatar,profile_background,unique_person_proof,contacts,blocked_users,achievements,external_achievements,message_activity_events,phone_is_verified,referred_by"
        );
        println!(
            "AFTER_FIELD_ORDER=encoder_version,bio,username,display_name,avatar,profile_background,unique_person_proof,contacts,blocked_users,achievements,external_achievements,message_activity_events,phone_is_verified,referred_by"
        );
        println!("ENCODER_VERSION={PII_ENCODER_VERSION}");
        println!("BEFORE_PRE_STATE_HASH={}", hex::encode(before_pre));
        println!("AFTER_PRE_STATE_HASH={}", hex::encode(after_pre));
        println!("BEFORE_POST_STATE_HASH={}", hex::encode(before_post));
        println!("AFTER_POST_STATE_HASH={}", hex::encode(after_post));

        assert_ne!(before_pre, after_pre);
        assert_ne!(before_post, after_post);
    }

    #[test]
    fn trust_root_label_matches_build_profile() {
        let trust_root_key_id = zombie_core::nns_keys::active_key_id();
        println!("ACTIVE_TRUST_ROOT_KEY_ID={trust_root_key_id}");

        #[cfg(feature = "local-replica")]
        assert_eq!(trust_root_key_id, "local-dev");

        #[cfg(not(feature = "local-replica"))]
        assert_eq!(trust_root_key_id, "mainnet");
    }
}
