use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use types::{BuildVersion, CanisterId};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// The WRITE/STORE authority (G-ruled §2): the set of principals allowed to
    /// call `store`. In OpenChat this is the `local_user_index` canister(s) that
    /// orchestrate the pre-uninstall export. No arbitrary public writes.
    /// Explicit + environment-driven (§5) — supplied at install time, never
    /// hard-coded into product logic.
    pub authorized_principals: Vec<Principal>,
    pub cycles_dispenser_canister_id: CanisterId,
    pub wasm_version: BuildVersion,
    pub test_mode: bool,
}
