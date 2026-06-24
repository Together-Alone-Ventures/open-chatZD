use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use types::SuccessOnly;

/// Controller-gated (§5, env/operator-driven): add a principal to the WRITE/STORE
/// authority. In OpenChat this is how the dynamically-created `local_user_index`
/// canister id is granted export rights after it exists, without hard-coding it
/// into product logic.
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub principal: Principal,
}

pub type Response = SuccessOnly;
