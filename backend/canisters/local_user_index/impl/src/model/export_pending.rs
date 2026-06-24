use crate::memory::{Memory, get_export_pending_memory};
use candid::{CandidType, Principal};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use types::{CanisterId, TimestampMillis, UserId};

/// Durable, upgrade-surviving record of a user whose deletion is blocked on the
/// pre-uninstall CVDR export (P2 remediation, G option (c)). Lives in its OWN
/// stable-memory `StableBTreeMap` (a dedicated `MemoryId`), NOT in the heap `Data`
/// blob or the job queue — so a `local_user_index` upgrade cannot lose it. A
/// record is created on the FIRST export failure and only removed once the export
/// succeeds (then uninstall proceeds). Retained-copy-first: while a record exists,
/// the user canister stays installed/tombstoned/finalized and is never uninstalled.
#[derive(Serialize, Deserialize)]
pub struct ExportPending {
    #[serde(skip, default = "init_map")]
    map: StableBTreeMap<Principal, ExportPendingRecord, Memory>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ExportStatus {
    /// Inside the bounded fast-retry window (still re-queued in `users_to_delete_queue`).
    Retrying,
    /// Bounded window exhausted — durably parked; the slow drain job owns it.
    Parked,
    /// Export succeeded; uninstall is the only remaining step (transient).
    ExportedUninstallPending,
}

/// G's minimum durable fields. No plaintext PII — only canister principals, the
/// receipt id, timestamps, attempt count, a sanitized error class, and status.
///
/// #13 completion snapshot: once a record is `ExportedUninstallPending`, deletion
/// must be completable from THIS record alone — without ever calling the (possibly
/// already-destroyed) user canister again. The only completion inputs not already
/// derivable from `user_id` are the deletion-notification targets, captured once at
/// the export/finalization point in `canisters_to_notify`. This is an internal
/// completion snapshot, NOT a data store: still no handle/username/email/display
/// name/profile PII/receipt body.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ExportPendingRecord {
    pub user_id: UserId,
    pub user_canister_id: CanisterId,
    /// 32-byte receipt id (hex-able), if the export got far enough to learn it.
    pub receipt_id: Option<Vec<u8>>,
    pub receipts_canister_id: Option<CanisterId>,
    /// #13 completion snapshot: the groups/communities that must receive a
    /// `NotifyOfUserDeleted` once deletion completes. Captured BEFORE uninstall (so
    /// recovery never re-reads the destroyed canister) and treated as best-effort /
    /// idempotent on use per existing delete-user semantics (a target that no longer
    /// exists is simply skipped). NEVER exposed publicly or in metrics/logs.
    pub canisters_to_notify: Vec<CanisterId>,
    pub attempt: u32,
    pub first_failed_at: TimestampMillis,
    pub last_failed_at: TimestampMillis,
    /// Sanitized error class/category — never a PII-bearing message.
    pub last_error_class: String,
    pub next_retry_at: TimestampMillis,
    pub status: ExportStatus,
}

impl Storable for ExportPendingRecord {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("ExportPendingRecord encode"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(&self).expect("ExportPendingRecord encode")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("ExportPendingRecord decode")
    }
    const BOUND: Bound = Bound::Unbounded;
}

impl ExportPending {
    pub fn upsert(&mut self, record: ExportPendingRecord) {
        self.map.insert(record.user_canister_id, record);
    }

    pub fn get(&self, user_canister_id: &CanisterId) -> Option<ExportPendingRecord> {
        self.map.get(user_canister_id)
    }

    pub fn remove(&mut self, user_canister_id: &CanisterId) -> Option<ExportPendingRecord> {
        self.map.remove(user_canister_id)
    }

    pub fn len(&self) -> u64 {
        self.map.len()
    }

    /// Count of records in the `ExportedUninstallPending` state (export confirmed,
    /// only uninstall remains). Exposed as an aggregate metric for observability.
    pub fn uninstall_pending_count(&self) -> u64 {
        self.map
            .iter()
            .filter(|e| e.value().status == ExportStatus::ExportedUninstallPending)
            .count() as u64
    }

    /// Drain-eligible records whose `next_retry_at` is due — BOTH `Parked` (export
    /// not yet confirmed) and `ExportedUninstallPending` (export done, uninstall
    /// pending) are self-healed by the drain.
    pub fn due_for_retry(&self, now: TimestampMillis) -> Vec<ExportPendingRecord> {
        self.map
            .iter()
            .map(|e| e.value())
            .filter(|r| is_drain_eligible(&r.status) && r.next_retry_at <= now)
            .collect()
    }

    pub fn has_retryable(&self) -> bool {
        self.map.iter().any(|e| is_drain_eligible(&e.value().status))
    }
}

/// A record the slow drain job is responsible for retrying: parked (export not
/// confirmed) or exported-and-uninstall-pending. `Retrying` is owned by the fast
/// queue, not the drain.
fn is_drain_eligible(status: &ExportStatus) -> bool {
    matches!(status, ExportStatus::Parked | ExportStatus::ExportedUninstallPending)
}

impl Default for ExportPending {
    fn default() -> Self {
        ExportPending { map: init_map() }
    }
}

fn init_map() -> StableBTreeMap<Principal, ExportPendingRecord, Memory> {
    StableBTreeMap::init(get_export_pending_memory())
}
