use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use types::{BuildVersion, CanisterId, Hash};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    // The wasm version running on this canister
    pub wasm_version: BuildVersion,
    // R-2: the module hash the deploy pipeline EXPECTS this index to run (gzip upload hash). An
    // ops-integrity guard only — never a trust input, never in a receipt preimage.
    pub expected_index_module_hash: Option<Hash>,
    pub user_index_canister_id: CanisterId,
    pub group_index_canister_id: CanisterId,
    pub notifications_index_canister_id: CanisterId,
    pub identity_canister_id: CanisterId,
    pub proposals_bot_canister_id: CanisterId,
    pub cycles_dispenser_canister_id: CanisterId,
    pub escrow_canister_id: CanisterId,
    pub event_relay_canister_id: CanisterId,
    pub online_users_canister_id: CanisterId,
    pub internet_identity_canister_id: CanisterId,
    pub website_canister_id: CanisterId,
    pub video_call_operators: Vec<Principal>,
    pub oc_secret_key_der: Vec<u8>,
    pub rng_seed: [u8; 32],
    pub ic_root_key: Vec<u8>,
    pub openai_api_key: Option<String>,
    pub test_mode: bool,
}
