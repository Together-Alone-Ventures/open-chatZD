//! TTL-purge prepared-but-not-deleted CVDR drafts (spec §11.4).

use crate::{RuntimeState, mutate_state, read_state};
use constants::HOUR_IN_MS;
use ic_cdk_timers::TimerId;
use std::cell::Cell;
use std::time::Duration;
use tracing::info;

thread_local! {
    static TIMER_ID: Cell<Option<TimerId>> = Cell::default();
}

pub(crate) fn start_if_required(state: &RuntimeState) {
    if TIMER_ID.get().is_some() {
        return;
    }
    // Always schedule — prepared drafts may appear later; cheap no-op when empty.
    let _ = state;
    let timer_id = ic_cdk_timers::set_timer_interval(Duration::from_millis(HOUR_IN_MS), || {
        run();
    });
    TIMER_ID.set(Some(timer_id));
}

fn run() {
    let purged = mutate_state(|state| {
        let now = state.env.now();
        state.data.cvdr.purge_expired_prepared(now)
    });
    if purged > 0 {
        info!(purged, "purged expired Prepared CVDR drafts");
    }
    // Keep timer alive even if idle.
    let _ = read_state(|s| s.data.cvdr.draft_count());
}
