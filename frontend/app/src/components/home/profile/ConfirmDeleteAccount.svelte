<script lang="ts">
    import { AuthProvider, i18nKey, OpenChat } from "openchat-client";
    import ModalContent from "../../ModalContent.svelte";
    import Overlay from "../../Overlay.svelte";
    import Translatable from "../../Translatable.svelte";
    import { interpolate } from "../../../i18n/i18n";
    import { _ } from "svelte-i18n";
    import Markdown from "@shared_components/Markdown.svelte";
    import ButtonGroup from "../../ButtonGroup.svelte";
    import Button from "../../Button.svelte";
    import Checkbox from "../../Checkbox.svelte";
    import { getContext, onMount } from "svelte";
    import { toastStore } from "../../../stores/toast";
    import ReAuthenticate from "./ReAuthenticate.svelte";
    import type { DelegationChain, ECDSAKeyIdentity } from "@icp-sdk/core/identity";

    const client = getContext<OpenChat>("client");

    interface Props {
        deleting: boolean;
        onClose: () => void;
    }

    let { deleting = $bindable(), onClose }: Props = $props();

    // MKTd02 full-delete flow:
    //   intro → confirm (deliberate action) → reauth → processing (A→B→C)
    //         → receipt (download/copy) → deleting (guarded deleteCurrentUser)
    // `recovering` is the initial state while we detect an interrupted deletion.
    type Step =
        | "recovering"
        | "intro"
        | "confirm"
        | "reauth"
        | "processing"
        | "receipt"
        | "deleting"
        | "error";

    let step = $state<Step>("recovering");
    let confirmed = $state(false);
    let receiptIdHex = $state<string | undefined>(undefined);
    let authKey = $state<ECDSAKeyIdentity | undefined>(undefined);
    let authDelegation = $state<DelegationChain | undefined>(undefined);
    let errorMsg = $state<string | undefined>(undefined);
    let downloading = $state(false);

    // Subpart C — recovery: detect an interrupted deletion on open and resume,
    // never resurrecting normal use from a tombstoned-pending state.
    onMount(async () => {
        try {
            const state = await client.mktdPendingDeletionState();
            if (state.tombstoned && !state.pending) {
                // Phase C already finalized; teardown didn't run — show the receipt.
                receiptIdHex = client.mktdStoredReceiptIdHex();
                step = "receipt";
            } else if (state.tombstoned && state.pending) {
                // Phase A done but not C — resume B→C straight away (no re-run of A).
                await runFlow();
            } else {
                step = "intro";
            }
        } catch {
            // If we can't read the state, start from the top; the orchestration and
            // guard are idempotent and will not double-run destructive calls.
            step = "intro";
        }
    });

    // Subpart A — run A→B→C against the user's own canister. Idempotent/resume-safe.
    async function runFlow() {
        step = "processing";
        errorMsg = undefined;
        const res = await client.mktdRunDeletionReceiptFlow();
        if (res.kind === "success") {
            receiptIdHex = res.receiptIdHex;
            step = "receipt";
        } else {
            errorMsg = res.error;
            step = "error";
        }
    }

    function onReauthSuccess(detail: {
        key: ECDSAKeyIdentity;
        delegation: DelegationChain;
        provider: AuthProvider;
    }) {
        authKey = detail.key;
        authDelegation = detail.delegation;
        if (receiptIdHex !== undefined) {
            // Re-auth was requested only to run the final teardown on a receipt we
            // already hold (recovery-after-C).
            void finishDeletion();
        } else {
            // Deliberate confirmation done + re-authenticated → Phase A is the point
            // of no return; PII is tombstoned the moment the flow starts.
            void runFlow();
        }
    }

    // Subpart B — download the finalized receipt JSON from the live user canister
    // via the P1f /mktd_receipt route. A download failure never undoes deletion.
    async function downloadReceipt() {
        if (receiptIdHex === undefined) return;
        const url = client.mktdReceiptDownloadUrl(receiptIdHex);
        if (url === undefined) return;
        downloading = true;
        try {
            const resp = await fetch(url);
            if (!resp.ok) {
                throw new Error(`status ${resp.status}`);
            }
            const buf = await resp.arrayBuffer();
            const blobUrl = URL.createObjectURL(new Blob([buf], { type: "application/json" }));
            const anchor = document.createElement("a");
            anchor.href = blobUrl;
            anchor.download = `mktd-receipt-${receiptIdHex}.json`;
            document.body.appendChild(anchor);
            anchor.click();
            anchor.remove();
            URL.revokeObjectURL(blobUrl);
        } catch {
            toastStore.showFailureToast(i18nKey("danger.mktd.downloadFailed"));
        } finally {
            downloading = false;
        }
    }

    function copyReceiptId() {
        if (receiptIdHex === undefined) return;
        navigator.clipboard.writeText(receiptIdHex).then(
            () => toastStore.showSuccessToast(i18nKey("danger.mktd.copied")),
            () => toastStore.showFailureToast(i18nKey("danger.deleteAccountFailed")),
        );
    }

    // Final, guarded teardown — only reachable from the receipt screen, and the
    // client re-checks finalized canister state before running anything destructive.
    async function finishDeletion() {
        if (authKey === undefined || authDelegation === undefined) {
            // Recovery-after-C: we need a fresh delegation for the identity-level
            // teardown. Route through re-auth, then come back here.
            step = "reauth";
            return;
        }
        deleting = true;
        step = "deleting";
        try {
            const success = await client.deleteCurrentUser(
                authKey.getKeyPair(),
                authDelegation.toJSON(),
            );
            if (!success) {
                toastStore.showFailureToast(i18nKey("danger.deleteAccountFailed"));
                step = "receipt";
            } else {
                onClose();
            }
        } finally {
            deleting = false;
        }
    }

    const messageKeys = ["danger.mktd.msg1", "danger.mktd.msg2", "danger.mktd.msg3"];
    const scopeKeys = ["danger.mktd.scope1", "danger.mktd.scope2", "danger.mktd.scope3"];
</script>

<Overlay>
    <ModalContent
        closeIcon={step === "intro" || step === "confirm" || step === "receipt"}
        {onClose}>
        {#snippet header()}
            {#if step === "receipt" || step === "deleting"}
                <Translatable resourceKey={i18nKey("danger.mktd.receiptTitle")} />
            {:else if step === "confirm"}
                <Translatable resourceKey={i18nKey("danger.mktd.finalConfirmTitle")} />
            {:else}
                <Translatable resourceKey={i18nKey("danger.mktd.introTitle")} />
            {/if}
        {/snippet}
        {#snippet body()}
            <div class="delete-body">
                {#if step === "recovering" || step === "processing" || step === "deleting"}
                    <p class="processing">
                        <Translatable resourceKey={i18nKey("danger.mktd.processing")} />
                    </p>
                {:else if step === "intro"}
                    <Markdown
                        inline={false}
                        text={interpolate($_, i18nKey("danger.mktd.intro"))} />
                {:else if step === "confirm"}
                    <p class="final-confirm">
                        <Translatable resourceKey={i18nKey("danger.mktd.finalConfirm")} />
                    </p>
                    <Checkbox
                        id="mktd-delete-confirm"
                        bind:checked={confirmed}
                        label={i18nKey("danger.mktd.confirmCheckbox")} />
                {:else if step === "reauth"}
                    <ReAuthenticate
                        onSuccess={onReauthSuccess}
                        message={i18nKey("danger.reauth")} />
                {:else if step === "error"}
                    <p class="error">
                        <Translatable resourceKey={i18nKey("danger.mktd.flowError")} />
                    </p>
                    {#if errorMsg !== undefined}
                        <p class="error-detail">{errorMsg}</p>
                    {/if}
                {:else if step === "receipt"}
                    <p class="underway">
                        <Translatable resourceKey={i18nKey("danger.mktd.receiptUnderway")} />
                    </p>

                    {#if receiptIdHex !== undefined}
                        <div class="receipt-id">
                            <span class="label">
                                <Translatable resourceKey={i18nKey("danger.mktd.receiptIdLabel")} />
                            </span>
                            <code class="value">{receiptIdHex}</code>
                        </div>
                    {/if}

                    <ButtonGroup>
                        <Button
                            small
                            disabled={receiptIdHex === undefined || downloading}
                            loading={downloading}
                            onClick={downloadReceipt}>
                            <Translatable
                                resourceKey={i18nKey(
                                    downloading ? "danger.mktd.downloading" : "danger.mktd.download",
                                )} />
                        </Button>
                        <Button
                            small
                            secondary
                            disabled={receiptIdHex === undefined}
                            onClick={copyReceiptId}>
                            <Translatable resourceKey={i18nKey("danger.mktd.copyId")} />
                        </Button>
                    </ButtonGroup>

                    <ul class="messaging">
                        {#each messageKeys as key (key)}
                            <li><Translatable resourceKey={i18nKey(key)} /></li>
                        {/each}
                    </ul>

                    <div class="scope">
                        <h4><Translatable resourceKey={i18nKey("danger.mktd.scopeTitle")} /></h4>
                        <ul>
                            {#each scopeKeys as key (key)}
                                <li><Translatable resourceKey={i18nKey(key)} /></li>
                            {/each}
                        </ul>
                    </div>
                {/if}
            </div>
        {/snippet}
        {#snippet footer()}
            <ButtonGroup>
                {#if step === "intro"}
                    <Button small secondary onClick={onClose}>
                        <Translatable resourceKey={i18nKey("cancel")} />
                    </Button>
                    <Button small danger onClick={() => (step = "confirm")}>
                        <Translatable resourceKey={i18nKey("danger.mktd.continue")} />
                    </Button>
                {:else if step === "confirm"}
                    <Button small secondary onClick={() => (step = "intro")}>
                        <Translatable resourceKey={i18nKey("danger.mktd.back")} />
                    </Button>
                    <Button
                        small
                        danger
                        disabled={!confirmed}
                        onClick={() => (step = "reauth")}>
                        <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
                    </Button>
                {:else if step === "error"}
                    <Button small secondary onClick={onClose}>
                        <Translatable resourceKey={i18nKey("cancel")} />
                    </Button>
                    <Button small danger onClick={runFlow}>
                        <Translatable resourceKey={i18nKey("danger.mktd.retry")} />
                    </Button>
                {:else if step === "receipt"}
                    <Button
                        small
                        danger
                        disabled={deleting}
                        loading={deleting}
                        onClick={finishDeletion}>
                        <Translatable
                            resourceKey={i18nKey(
                                deleting ? "danger.mktd.finishing" : "danger.mktd.finish",
                            )} />
                    </Button>
                {/if}
            </ButtonGroup>
        {/snippet}
    </ModalContent>
</Overlay>

<style lang="scss">
    .delete-body {
        display: flex;
        flex-direction: column;
        gap: $sp4;
    }
    .underway,
    .error {
        font-weight: 700;
    }
    .error-detail {
        color: var(--txt-light);
        word-break: break-word;
        @include font(book, normal, fs-70);
    }
    .receipt-id {
        display: flex;
        flex-direction: column;
        gap: $sp2;
        .label {
            color: var(--txt-light);
            @include font(book, normal, fs-70);
        }
        .value {
            word-break: break-all;
            @include font(book, normal, fs-80);
        }
    }
    .messaging,
    .scope ul {
        margin: 0;
        padding-left: $sp5;
        display: flex;
        flex-direction: column;
        gap: $sp2;
    }
    .scope h4 {
        margin-bottom: $sp2;
    }
</style>
