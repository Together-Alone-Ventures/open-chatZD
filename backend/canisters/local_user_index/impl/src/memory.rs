use ic_stable_structures::{
    DefaultMemoryImpl, Memory as MemoryTrait,
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
};
use std::collections::BTreeMap;

const UPGRADES: MemoryId = MemoryId::new(0);
const STABLE_MEMORY_MAP: MemoryId = MemoryId::new(3);
// P2 remediation: durable, upgrade-surviving parked-export records. BANKED under v5
// (CVDR-on-Index); retained so any pre-existing records still deserialize.
const EXPORT_PENDING: MemoryId = MemoryId::new(4);
// RETIRED (slots reserved, never reused): the legacy single-CVDR `ReleasedCvdr` store (5) and its
// secondary index (6) were removed with the finalization rework. Their MemoryIds stay reserved so
// slot numbering never shifts; `memory_sizes()` still reports them.
// const CVDR_STORE: MemoryId = MemoryId::new(5);
// const CVDR_INDEX: MemoryId = MemoryId::new(6);
// v5 CVDR-on-Index: in-flight durable deletion DRAFT, keyed by user_canister_id, that
// survives upgrades mid-flight and drives forward-only recovery (no upgrade-trapping lock).
const CVDR_DRAFT: MemoryId = MemoryId::new(7);
// CVDR finalization rework (spec §4) — frozen-package storage, extends the 8-slot map (0–7).
// Spreadsheet update queued separately. Slots FROZEN per spec §4:
//   8  = frozen-package StableLog INDEX memory
//   9  = frozen-package StableLog DATA memory
//   10 = primary   StableBTreeMap: receipt_id -> log offset
//   11 = secondary StableBTreeMap: (record_id, deletion_seq) -> receipt_id (access-gated)
const CVDR_FROZEN_LOG_INDEX: MemoryId = MemoryId::new(8);
const CVDR_FROZEN_LOG_DATA: MemoryId = MemoryId::new(9);
const CVDR_FROZEN_PRIMARY: MemoryId = MemoryId::new(10);
const CVDR_FROZEN_SECONDARY: MemoryId = MemoryId::new(11);

pub type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: MemoryManager<DefaultMemoryImpl>
        = MemoryManager::init_with_bucket_size(DefaultMemoryImpl::default(), 16);
}

pub fn get_upgrades_memory() -> Memory {
    get_memory(UPGRADES)
}

pub fn get_stable_memory_map_memory() -> Memory {
    get_memory(STABLE_MEMORY_MAP)
}

pub fn get_export_pending_memory() -> Memory {
    get_memory(EXPORT_PENDING)
}

pub fn get_cvdr_draft_memory() -> Memory {
    get_memory(CVDR_DRAFT)
}

pub fn get_cvdr_frozen_log_index_memory() -> Memory {
    get_memory(CVDR_FROZEN_LOG_INDEX)
}

pub fn get_cvdr_frozen_log_data_memory() -> Memory {
    get_memory(CVDR_FROZEN_LOG_DATA)
}

pub fn get_cvdr_frozen_primary_memory() -> Memory {
    get_memory(CVDR_FROZEN_PRIMARY)
}

pub fn get_cvdr_frozen_secondary_memory() -> Memory {
    get_memory(CVDR_FROZEN_SECONDARY)
}

pub fn memory_sizes() -> BTreeMap<u8, u64> {
    (0u8..=11).map(|id| (id, get_memory(MemoryId::new(id)).size())).collect()
}

fn get_memory(id: MemoryId) -> Memory {
    MEMORY_MANAGER.with(|m| m.get(id))
}
