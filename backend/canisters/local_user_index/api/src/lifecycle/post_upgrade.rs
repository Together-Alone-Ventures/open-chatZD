use candid::CandidType;
use serde::{Deserialize, Serialize};
use types::{BuildVersion, Hash};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub wasm_version: BuildVersion,
    // CVDR v5: this index's OWN newly-deployed module hash (gzip upload hash), supplied by the
    // deploy pipeline on every upgrade — refreshes the captured H_index executor provenance.
    pub executor_module_hash: Hash,
}
