use ic_stable_structures::{
    DefaultMemoryImpl, Memory as MemoryTrait,
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
};
use std::collections::BTreeMap;

const UPGRADES: MemoryId = MemoryId::new(0);
const STABLE_MEMORY_MAP: MemoryId = MemoryId::new(3);

pub type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: MemoryManager<DefaultMemoryImpl>
        = MemoryManager::init_with_bucket_size(DefaultMemoryImpl::default(), 2);
}

pub fn get_upgrades_memory() -> Memory {
    get_memory(UPGRADES)
}

pub fn get_stable_memory_map_memory() -> Memory {
    get_memory(STABLE_MEMORY_MAP)
}

pub fn memory_sizes() -> BTreeMap<u8, u64> {
    (0u8..=3).map(|id| (id, get_memory(MemoryId::new(id)).size())).collect()
}

fn get_memory(id: MemoryId) -> Memory {
    MEMORY_MANAGER.with(|m| m.get(id))
}

// RESERVED — never reuse: MemoryIds 100..=107.
// The retired user-canister MKTd02 engine (removed by R-3, suite-v5 retrofit) allocated these eight
// slots on this MemoryManager. User canisters upgraded from a wasm that carried the engine still hold
// those buckets; they are deliberately left unreclaimed and unread. Any new stable structure must
// pick an id outside this range.
#[allow(dead_code)]
const RETIRED_MKTD02_MEMORY_IDS: std::ops::RangeInclusive<u8> = 100..=107;
