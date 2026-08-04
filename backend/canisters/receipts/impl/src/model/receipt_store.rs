use crate::memory::{Memory, get_receipts_memory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Durable, post-uninstall receipt store (D4/O1 option 3). Keyed by the 32-byte
/// receipt id, holding the EXACT finalized-CVDR JSON bytes the user canister
/// exported. Bytes are stored verbatim and served verbatim (§3) — no
/// parse-then-reserialize, so a future serializer/dependency bump cannot drift
/// the served bytes.
#[derive(Serialize, Deserialize)]
pub struct ReceiptStore {
    #[serde(skip, default = "init_map")]
    map: StableBTreeMap<ReceiptId, ReceiptBytes, Memory>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreOutcome {
    /// Newly inserted.
    Stored,
    /// Same id, byte-identical content already present (idempotent OK).
    AlreadyExists,
    /// Same id, DIFFERENT content already present (hard reject).
    Conflict,
}

impl ReceiptStore {
    /// Idempotent store with dup-different-bytes hard reject (§2).
    pub fn put(&mut self, receipt_id: [u8; 32], bytes: Vec<u8>) -> StoreOutcome {
        match self.map.get(&ReceiptId(receipt_id)) {
            Some(existing) if existing.0 == bytes => StoreOutcome::AlreadyExists,
            Some(_) => StoreOutcome::Conflict,
            None => {
                self.map.insert(ReceiptId(receipt_id), ReceiptBytes(bytes));
                StoreOutcome::Stored
            }
        }
    }

    pub fn get(&self, receipt_id: &[u8; 32]) -> Option<Vec<u8>> {
        self.map.get(&ReceiptId(*receipt_id)).map(|v| v.0)
    }

    pub fn len(&self) -> u64 {
        self.map.len()
    }
}

impl Default for ReceiptStore {
    fn default() -> Self {
        ReceiptStore { map: init_map() }
    }
}

fn init_map() -> StableBTreeMap<ReceiptId, ReceiptBytes, Memory> {
    StableBTreeMap::init(get_receipts_memory())
}

/// 32-byte fixed-size key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReceiptId([u8; 32]);

impl Storable for ReceiptId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let mut id = [0u8; 32];
        id.copy_from_slice(&bytes);
        ReceiptId(id)
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

/// Raw finalized-receipt JSON bytes. The current canonical receipt JSON is well
/// under this bound; bump it (and re-test byte identity) if the receipt schema
/// grows.
#[derive(Clone)]
struct ReceiptBytes(Vec<u8>);

impl Storable for ReceiptBytes {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        ReceiptBytes(bytes.to_vec())
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 16_384,
        is_fixed_size: false,
    };
}
