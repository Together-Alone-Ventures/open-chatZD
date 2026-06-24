use crate::read_state;

/// Controller-gated config surface (§5): only a canister controller may mutate
/// the export authority set.
pub fn caller_is_controller() -> Result<(), String> {
    if ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        Ok(())
    } else {
        Err("Caller is not a controller of the receipts canister".to_string())
    }
}

/// WRITE-gating (§2): only the configured export authority may `store`. An
/// unauthorized caller is rejected at the message boundary (the `#[update]`
/// guard turns this `Err` into a canister reject), so no fake receipts can be
/// spammed into the durable store.
pub fn caller_is_authorized() -> Result<(), String> {
    if read_state(|state| state.is_caller_authorized()) {
        Ok(())
    } else {
        Err("Caller is not an authorized receipts export principal".to_string())
    }
}
