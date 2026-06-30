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
// v5 CVDR-on-Index: released CVDRs keyed by 32-byte receipt_id (the public fetch path).
const CVDR_STORE: MemoryId = MemoryId::new(5);
// v5 CVDR-on-Index: secondary index (record_id, deletion_seq) -> receipt_id (gated lookup).
const CVDR_INDEX: MemoryId = MemoryId::new(6);
// v5 CVDR-on-Index: in-flight durable deletion DRAFT, keyed by user_canister_id, that
// survives upgrades mid-flight and drives forward-only recovery (no upgrade-trapping lock).
const CVDR_DRAFT: MemoryId = MemoryId::new(7);

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

pub fn get_cvdr_store_memory() -> Memory {
    get_memory(CVDR_STORE)
}

pub fn get_cvdr_index_memory() -> Memory {
    get_memory(CVDR_INDEX)
}

pub fn get_cvdr_draft_memory() -> Memory {
    get_memory(CVDR_DRAFT)
}

pub fn memory_sizes() -> BTreeMap<u8, u64> {
    (0u8..=7).map(|id| (id, get_memory(MemoryId::new(id)).size())).collect()
}

fn get_memory(id: MemoryId) -> Memory {
    MEMORY_MANAGER.with(|m| m.get(id))
}
