// MKTd02 full-delete (OpenChatZD) — frontend domain types for the owner-driven
// A→B→C deletion-receipt flow against the user's OWN canister.
//
// Phase A: mktd_execute_deletion  (update) — tombstone PII, take finalization lock,
//          return the pending receipt id. POINT OF NO RETURN.
// Phase B: mktd_pending_certificate (ingress query) — read the BLS certificate.
// Phase C: mktd_finalize_deletion  (update) — embed cert, finalize; this is what
//          sets the durable finalized receipt id the P2 backend export reads.
//
// The frontend never stores to the receipts canister — P2 owns the durable store at
// the backend teardown seam. The frontend only finalizes, displays, and downloads
// the receipt from the live user canister, then triggers the existing teardown.

/** Snapshot of the user canister's MKTd deletion state, used to drive the flow and
 * the serialization guard. Derived from `mktd_pending_deletion_state`.
 * - not started:        tombstoned=false, pending=false
 * - Phase A done, not C: tombstoned=true,  pending=true   (finalization lock held)
 * - Phase C finalized:   tombstoned=true,  pending=false  (receipt durable) */
export type MktdDeletionState = {
    pending: boolean;
    tombstoned: boolean;
    receiptId?: Uint8Array;
};

/** True once Phase C has finalized the receipt — the ONLY state in which the
 * destructive `deleteCurrentUser` teardown may run (the serialization guard). */
export function mktdIsFinalized(state: MktdDeletionState): boolean {
    return state.tombstoned && !state.pending;
}

/** True when Phase A has run but Phase C has not — tombstoned-but-pending. Normal
 * app/profile use must be blocked and the user routed to "finalize + download". */
export function mktdIsTombstonedPending(state: MktdDeletionState): boolean {
    return state.tombstoned && state.pending;
}

/** Shell-level deletion-corridor status (G ruling b). */
export type MktdCorridorStatus = "unknown" | "clear" | "blocked";

/** Pure decision for the shell-level corridor guard, given the one-shot deletion
 * state read (or `undefined` when that read FAILED / is unknown).
 *
 * The fail-closed distinction lives here and is deliberately narrow:
 * - `undefined` (read failed / status unknown) → "unknown": the shell must NOT trap a
 *   normal, non-deleting user behind a transient error, so this never blocks;
 * - confirmed tombstoned (pending OR finalized) → "blocked": steer into the recovery
 *   corridor — fail-closed applies ONLY to confirmed deletion state;
 * - confirmed not-tombstoned → "clear": normal app. */
export function mktdCorridorStatus(state: MktdDeletionState | undefined): MktdCorridorStatus {
    if (state === undefined) {
        return "unknown";
    }
    return state.tombstoned ? "blocked" : "clear";
}

export type MktdExecuteDeletionResponse =
    | { kind: "success"; receiptId: Uint8Array }
    | { kind: "error"; error: string };

export type MktdPendingCertificate = {
    receiptId: Uint8Array;
    certifiedCommitment: Uint8Array;
    certificate: Uint8Array;
};

export type MktdPendingCertificateResponse =
    | { kind: "success"; certificate: MktdPendingCertificate }
    | { kind: "not_pending" };

export type MktdFinalizeDeletionResponse = { kind: "success" } | { kind: "error"; error: string };
