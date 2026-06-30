use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use types::{BuildVersion, CanisterId, Hash};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    // The wasm version running on this canister
    pub wasm_version: BuildVersion,
    // CVDR v5: this index's OWN deployed module hash (gzip upload hash), supplied by the deploy
    // pipeline — the captured H_index executor provenance. Not read from inside the canister,
    // not computed from receipt fields.
    pub executor_module_hash: Hash,
    pub user_index_canister_id: CanisterId,
    pub group_index_canister_id: CanisterId,
    pub notifications_index_canister_id: CanisterId,
    pub identity_canister_id: CanisterId,
    pub proposals_bot_canister_id: CanisterId,
    pub cycles_dispenser_canister_id: CanisterId,
    pub escrow_canister_id: CanisterId,
    pub event_relay_canister_id: CanisterId,
    pub online_users_canister_id: CanisterId,
    // P2: durable receipts canister for the pre-uninstall CVDR export. `None`
    // disables export (retained-copy-first still blocks uninstall if a finalized
    // receipt exists but there is nowhere to export it). Explicit + env-driven (§5).
    pub receipts_canister_id: Option<CanisterId>,
    pub internet_identity_canister_id: CanisterId,
    pub website_canister_id: CanisterId,
    pub video_call_operators: Vec<Principal>,
    pub oc_secret_key_der: Vec<u8>,
    pub rng_seed: [u8; 32],
    pub ic_root_key: Vec<u8>,
    pub openai_api_key: Option<String>,
    pub test_mode: bool,
}
