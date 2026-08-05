# OpenChatZD — Anonymous CVDR recovery walkthrough (Stef delta evidence)

**Date:** 2026-08-06  
**Product tip:** `antek` @ current HEAD (A ordering: `deletionStarted` before delete)  
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

- [ ] Recording shows anon identity (no authenticated chrome)
- [ ] Auto-poll starts without re-login
- [ ] CVDR file download completes successfully
- [ ] No permanent “stuck delayed” without a download when Available

## Attach

- Recording file path / link: _(fill after capture)_
- Receipt id (first 8 hex): _(fill)_
- Product commit: `git -C open-chatZD rev-parse HEAD`

## Related code

- `frontend/app/src/components_shared/CvdrAnonymousRecovery.svelte`
- `runDeletionWithPreflightRecoveryFlag` in `openchat-shared/src/domain/cvdr.ts`
- Desktop/mobile `ConfirmDeleteAccount.svelte` → `handoffToAnonymousRecovery`
