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

/// Borrow the canister's single `MemoryManager` to hand to the MKTd02 engine.
///
/// MKTd02 (`mktd02::init` / `mktd02::on_post_upgrade`) is wired onto this SAME
/// manager so its 8 reserved slots (`base_memory_id`..=`base_memory_id + 7`,
/// i.e. 100..=107 — see [`crate::mktd`]) live alongside the host's slots
/// 0 (UPGRADES) and 3 (STABLE_MEMORY_MAP). The two slot sets are disjoint;
/// `memory_sizes()` reporting (0..=3) is unaffected.
pub fn with_memory_manager<R>(f: impl FnOnce(&MemoryManager<DefaultMemoryImpl>) -> R) -> R {
    MEMORY_MANAGER.with(|m| f(m))
}
