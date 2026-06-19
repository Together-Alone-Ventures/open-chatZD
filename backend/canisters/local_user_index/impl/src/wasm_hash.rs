use std::io;
use types::Hash;

/// Hash of the canister module **exactly as submitted to `install_code`** — the
/// SHA-256 of the (gzip) upload bytes, with NO decompression.
///
/// The IC computes a canister's on-chain `module_hash` over the uploaded module
/// bytes; for a gzip-installed canister that is the hash of the gzip, not of the
/// decompressed wasm. CVDR-Verify V3 reads that on-chain value via
/// `read_state_canister_info(_, "module_hash")` and compares it directly to
/// `receipt.module_hash` (no decompression), so this cached value must be the
/// upload hash to match a live read.
pub(crate) fn deployed_module_hash(wasm: &[u8]) -> io::Result<Hash> {
    Ok(sha256::sha256(wasm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    #[test]
    fn deployed_module_hash_is_the_upload_hash_not_the_decompressed_hash() {
        let module = b"deployed wasm module";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(module).unwrap();
        let compressed = encoder.finish().unwrap();

        // On-chain `module_hash` convention: SHA-256 of the bytes submitted to
        // install_code (the gzip upload), which is what CVDR-Verify V3 reads.
        assert_eq!(deployed_module_hash(&compressed).unwrap(), sha256::sha256(&compressed));
        // It must NOT be the hash of the decompressed module (the old P1c value).
        assert_ne!(deployed_module_hash(&compressed).unwrap(), sha256::sha256(module));
    }
}
