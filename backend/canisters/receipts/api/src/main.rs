// The receipts canister uses msgpack for its c2c `store` endpoint and a raw
// `http_request` route for public read-by-id, so it has no Candid-typed update
// surface to export here. `can.did` is therefore not generated from this binary
// for the P2 slice (local-first, no `dfx deploy`); see the P2 report. Kept as a
// trivial binary so `cargo run -p receipts_canister` and workspace builds stay
// green, matching the api/main.rs convention of the other canisters.
fn main() {}
