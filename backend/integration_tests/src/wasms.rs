use crate::utils::local_bin;
use lazy_static::lazy_static;
use std::fs::File;
use std::io::Read;
use types::{BuildVersion, CanisterWasm};

lazy_static! {
    pub static ref AIRDROP_BOT: CanisterWasm = get_canister_wasm("airdrop_bot");
    pub static ref COMMUNITY: CanisterWasm = get_canister_wasm("community");
    pub static ref CYCLES_DISPENSER: CanisterWasm = get_canister_wasm("cycles_dispenser");
    pub static ref ESCROW: CanisterWasm = get_canister_wasm("escrow");
    pub static ref EVENT_RELAY: CanisterWasm = get_canister_wasm("event_relay");
    pub static ref EVENT_STORE: CanisterWasm = get_canister_wasm("event_store");
    pub static ref GROUP: CanisterWasm = get_canister_wasm("group");
    pub static ref GROUP_INDEX: CanisterWasm = get_canister_wasm("group_index");
    pub static ref ICP_LEDGER: CanisterWasm = get_canister_wasm("icp_ledger");
    pub static ref ICRC_LEDGER: CanisterWasm = get_canister_wasm("icrc_ledger");
    pub static ref IDENTITY: CanisterWasm = get_canister_wasm("identity");
    pub static ref LOCAL_USER_INDEX: CanisterWasm = get_canister_wasm("local_user_index");
    pub static ref NOTIFICATIONS_INDEX: CanisterWasm = get_canister_wasm("notifications_index");
    pub static ref ONLINE_USERS: CanisterWasm = get_canister_wasm("online_users");
    pub static ref OPENCHAT_INSTALLER: CanisterWasm = get_canister_wasm("openchat_installer");
    pub static ref PROPOSALS_BOT: CanisterWasm = get_canister_wasm("proposals_bot");
    pub static ref REGISTRY: CanisterWasm = get_canister_wasm("registry");
    pub static ref SIGN_IN_WITH_EMAIL: CanisterWasm = get_canister_wasm("sign_in_with_email");
    pub static ref SNS_WASM: CanisterWasm = get_canister_wasm("sns_wasm");
    pub static ref STORAGE_BUCKET: CanisterWasm = get_canister_wasm("storage_bucket");
    pub static ref STORAGE_INDEX: CanisterWasm = get_canister_wasm("storage_index");
    pub static ref TRANSLATIONS: CanisterWasm = get_canister_wasm("translations");
    pub static ref USER: CanisterWasm = get_canister_wasm("user");
    pub static ref USER_INDEX: CanisterWasm = get_canister_wasm("user_index");
}

/// Docker-built wasms of the pre-R-3 baseline (`c744de1`), used only by the two `#[ignore]`d upgrade
/// tests (`r3_upgrade_tests`, `cvdr_v2_upgrade_tests`). Not committed: copy them to
/// `wasms/baseline_c744de1/` (docs/dev/v5/BASELINE_c744de1.md §3.2), or run the manual
/// `upgrade-baseline` job in `.github/workflows/backend.yaml`, which builds them at c744de1.
///
/// Every load is checked against the SHA-256 recorded in that document, so the upgrade test can
/// only ever start from the exact `c744de1` all-canister-recipe artefacts.
pub fn baseline_c744de1(canister_name: &str) -> CanisterWasm {
    let expected = BASELINE_C744DE1_SHA256
        .iter()
        .find(|(name, _)| *name == canister_name)
        .unwrap_or_else(|| panic!("no recorded c744de1 baseline hash for `{canister_name}`"))
        .1;
    let wasm = get_canister_wasm(&format!("baseline_c744de1/{canister_name}"));
    let actual = hex::encode(sha256::sha256(&wasm.module));
    assert_eq!(
        actual, expected,
        "wasms/baseline_c744de1/{canister_name}.wasm.gz is not the c744de1 artefact recorded in \
         docs/dev/v5/BASELINE_c744de1.md §3.2"
    );
    wasm
}

/// `.wasm.gz` SHA-256 values from docs/dev/v5/BASELINE_c744de1.md §3.2 (all-canister Docker recipe).
const BASELINE_C744DE1_SHA256: [(&str, &str); 4] = [
    (
        "local_user_index",
        "83601cf8e1c35f487011c94eb9a02519a785755240b175462bbf04264396a601",
    ),
    (
        "openchat_installer",
        "81e2aa0b295122bd66efefcfc7dcda72c1910b9378486f2682e63d33d4177b64",
    ),
    ("user", "eec4762080e2bdd941cf3f9dcb8530665d6dfa9d2a8a9d81828ab4d066b80b45"),
    (
        "user_index",
        "f4c2d4c98fb9e5e56ab2f099859c35f237b3fbae3a8b66a858f11da53b053357",
    ),
];

fn get_canister_wasm(canister_name: &str) -> CanisterWasm {
    let wasm = read_file_from_local_bin(&format!("{canister_name}.wasm.gz"));

    CanisterWasm {
        version: BuildVersion::min(),
        module: wasm.into(),
    }
}

fn read_file_from_local_bin(file_name: &str) -> Vec<u8> {
    let mut file_path = local_bin();
    file_path.push(file_name);

    let mut file = File::open(&file_path).unwrap_or_else(|_| panic!("Failed to open file: {}", file_path.to_str().unwrap()));
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("Failed to read file");
    bytes
}
