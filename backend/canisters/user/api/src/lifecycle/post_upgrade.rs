use serde::{Deserialize, Serialize};
use types::BuildVersion;

#[derive(Serialize, Deserialize, Debug)]
pub struct Args {
    pub wasm_version: BuildVersion,
    /// SHA-256 of the deployed wasm for MKTd02 V3. Updated unconditionally on
    /// every upgrade. `None` is retained only for decoding older upgrade args.
    #[serde(default)]
    pub mktd_module_hash: Option<[u8; 32]>,
}
