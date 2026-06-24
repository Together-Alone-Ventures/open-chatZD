use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use types::SuccessOnly;

/// Controller-gated (§5): revoke a principal's WRITE/STORE rights — the symmetric
/// counterpart of `add_authorized_principal` (e.g. to retire a decommissioned
/// local_user_index). Idempotent: removing an absent principal still returns
/// `Success`.
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub principal: Principal,
}

pub type Response = SuccessOnly;
