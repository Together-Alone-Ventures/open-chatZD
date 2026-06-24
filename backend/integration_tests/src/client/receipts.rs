use crate::generate_msgpack_update_call;
use receipts_canister::*;

// Updates
generate_msgpack_update_call!(add_authorized_principal);
generate_msgpack_update_call!(remove_authorized_principal);
generate_msgpack_update_call!(store);
