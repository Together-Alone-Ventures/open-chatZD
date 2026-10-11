use candid::CandidType;
use serde::{Deserialize, Serialize};
use types::{BuildVersion, Hash};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub wasm_version: BuildVersion,
    // R-2: the module hash the deploy pipeline EXPECTS this index to run (gzip upload hash). An
    // ops-integrity guard only — compared with the certified module hash at evidence capture,
    // never a trust input, never in a receipt preimage. Optional so that a not-yet-upgraded
    // user_index (which still sends the retired `executor_module_hash`) can upgrade this index.
    pub expected_index_module_hash: Option<Hash>,
}
