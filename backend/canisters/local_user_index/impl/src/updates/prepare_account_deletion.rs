use crate::guards::caller_is_openchat_user;
use crate::model::cvdr::{self, CvdrDraft, DraftStage, receipt_id_prefix};
use crate::{mutate_state, read_state};
use canister_api_macros::update;
use canister_tracing_macros::trace;
use ic_cdk::management_canister::CanisterStatusArgs;
use local_user_index_canister::prepare_account_deletion::{Response::*, SuccessResult, *};
use rand::RngCore;
use tracing::info;
use types::{CanisterId, Empty, UserId};

#[update(guard = "caller_is_openchat_user", msgpack = true, candid = true)]
#[trace]
async fn prepare_account_deletion(_args: Args) -> Response {
    let (user_id, canister_id) = match read_state(|state| {
        let user = state.calling_user();
        let canister_id: CanisterId = user.user_id.into();
        // Local users only — remote users hit another LUI.
        if state.data.local_users.get(&user.user_id).is_none() {
            return Err(());
        }
        Ok((user.user_id, canister_id))
    }) {
        Ok(v) => v,
        Err(()) => return Error(oc_error_codes::OCErrorCode::TargetUserNotFound.into()),
    };

    // Idempotent re-issue while Prepared; refuse once deletion has progressed.
    if let Some(existing) = read_state(|state| state.data.cvdr.get_draft(&canister_id)) {
        return match existing.stage {
            DraftStage::Prepared => {
                info!(
                    receipt_id_prefix = %receipt_id_prefix(&existing.receipt_id),
                    "prepare_account_deletion re-issue"
                );
                Success(success_from_draft(&existing))
            }
            _ => AlreadyCommitted,
        };
    }

    match capture_prepared_draft(user_id, canister_id).await {
        Ok(draft) => {
            info!(
                receipt_id_prefix = %receipt_id_prefix(&draft.receipt_id),
                "prepare_account_deletion issued"
            );
            Success(success_from_draft(&draft))
        }
        Err(class) => UserCanisterUnavailable(class.to_string()),
    }
}

fn success_from_draft(draft: &CvdrDraft) -> SuccessResult {
    SuccessResult {
        receipt_id: hex::encode(draft.receipt_id),
        reveal_wire_json: String::from_utf8(draft.reveal_wire_json()).expect("RevealWire JSON is utf-8"),
    }
}

/// Shared capture used by prepare (stage `Prepared`). Extracted from the former cold
/// `capture_draft` path in `delete_users` — product deletes must prepare first.
pub(crate) async fn capture_prepared_draft(user_id: UserId, canister_id: CanisterId) -> Result<CvdrDraft, &'static str> {
    let canisters_to_notify = match user_canister_c2c_client::c2c_groups_and_communities(canister_id, &Empty {}).await {
        Ok(r) => r
            .groups
            .into_iter()
            .map(|g| g.into())
            .chain(r.communities.into_iter().map(|c| c.into()))
            .collect::<Vec<CanisterId>>(),
        Err(_) => return Err("groups_and_communities_unavailable"),
    };

    let module_hash_pre = match ic_cdk::management_canister::canister_status(&CanisterStatusArgs { canister_id }).await {
        Ok(status) => status.module_hash.unwrap_or_default(),
        Err(_) => return Err("canister_status_unavailable"),
    };

    let record_id = cvdr::record_id_for(user_id);
    let (deletion_seq, nonce, now, index_canister_id, executor_module_hash) = mutate_state(|state| {
        let seq = state.data.cvdr_next_deletion_seq;
        state.data.cvdr_next_deletion_seq = seq.saturating_add(1);
        let mut nonce = [0u8; 32];
        state.env.rng().fill_bytes(&mut nonce);
        (
            seq,
            nonce,
            state.env.now(),
            state.env.canister_id(),
            state.data.executor_module_hash.to_vec(),
        )
    });

    let h_user_pre = cvdr::h_user_pre(canister_id, &module_hash_pre);
    let h_index = cvdr::h_index(index_canister_id, &executor_module_hash);
    let commitment = cvdr::commitment(&record_id, deletion_seq, &h_user_pre, &h_index, canister_id);
    let receipt_id = cvdr::receipt_id_for(&record_id, deletion_seq, &nonce);
    let salt = utils::canister::get_random_seed().await;

    let draft = CvdrDraft {
        user_id,
        user_canister_id: canister_id,
        index_canister_id,
        record_id,
        deletion_seq,
        nonce,
        receipt_id,
        module_hash_pre,
        executor_module_hash,
        h_user_pre,
        h_index,
        commitment,
        salt,
        canisters_to_notify,
        uninstall_completed_at: 0,
        receipt_committed_at: 0,
        finalize_attempt: 0,
        finalize_last_attempt_at: 0,
        created_at: now,
        attempt: 0,
        stage: DraftStage::Prepared,
    };
    mutate_state(|state| state.data.cvdr.upsert_draft(draft.clone()));
    Ok(draft)
}
