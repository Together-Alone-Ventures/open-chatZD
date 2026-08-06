# OpenChatZD — Anonymous CVDR recovery walkthrough (Stef delta evidence)

**Date:** 2026-08-06  
**Product tip:** `antek` @ `(this commit — antek tip)` (A ordering: `deletionStarted` before delete)  
**Local stack:** LUI `ucwa4-rx777-77774-qaada-cai` · raw gateway `http://…raw.localhost:8080`  
**Purpose:** Capture one recording of **unauthenticated recovery continuing through successful CVDR delivery** (Stef completion-gate ask). Prior evidence only showed post-delete close-tab resume.

## Preconditions

- Frontend build from current `antek`
- Test account with prepared deletion path available (local / staging)
- Screen recorder ready (QuickTime / OBS)

## Script (record end-to-end in one take)

1. Sign in as the test user.
2. Start **Delete account** → prepare → download reveal → acknowledge → re-auth.
3. Confirm irreversible delete starts (UI shows deleting / polling).
4. **Do not** wait for CVDR in the authenticated modal.
5. Use **Handoff to anonymous recovery** (or logout / close tab after delete succeeded with `deletionStarted: true`).
6. Confirm identity is **anonymous** (`identityState.kind === "anon"`).
7. `CvdrAnonymousRecovery` must auto-detect the pending session (`deletionStarted: true`) and poll `/cvdr/<receipt_id>`.
8. Continue until **Available** → browser downloads the CVDR JSON (successful delivery).
9. Confirm session cleared from localStorage (`oc_cvdr_receipt_pending` gone).

## Pass criteria

- [x] Recording shows anon identity (no authenticated chrome)
- [x] Auto-poll starts without re-login
- [x] CVDR file download completes successfully
- [x] No permanent “stuck delayed” without a download when Available

## Attach

- Recording: delivered via Slack (same thread as Stef completion-gate handoff)
- Receipt id (full): `80af8b9c6ddb48150b762c32d80de16d176a09c6577ed447ece7015b3a0f1cf7`
- Receipt id (first 8 hex): `80af8b9c`
- Live URL (Available): `http://ucwa4-rx777-77774-qaada-cai.raw.localhost:8080/cvdr/80af8b9c6ddb48150b762c32d80de16d176a09c6577ed447ece7015b3a0f1cf7`
- Downloaded artefacts:
  - `/Users/antoine/Downloads/openchatzd-cvdr-80af8b9c.json` (3678 B — schema/certificate/receipt/witness)
  - `/Users/antoine/Downloads/openchatzd-reveal-80af8b9c.json` (159 B — reveal package)
- Product commit: `(this commit — antek tip)`

## Related code

- `frontend/app/src/components_shared/CvdrAnonymousRecovery.svelte`
- `runDeletionWithPreflightRecoveryFlag` in `openchat-shared/src/domain/cvdr.ts`
- Desktop/mobile `ConfirmDeleteAccount.svelte` → `handoffToAnonymousRecovery`
