<script lang="ts">
    import {
        AuthProvider,
        currentUserIdStore,
        cvdrSessionAwaitingDelivery,
        downloadBytesAsFile,
        i18nKey,
        OpenChat,
    } from "openchat-client";
    import ModalContent from "../../ModalContent.svelte";
    import Overlay from "../../Overlay.svelte";
    import Translatable from "../../Translatable.svelte";
    import { interpolate } from "../../../i18n/i18n";
    import { _ } from "svelte-i18n";
    import Markdown from "@shared_components/Markdown.svelte";
    import ButtonGroup from "../../ButtonGroup.svelte";
    import Button from "../../Button.svelte";
    import Checkbox from "../../Checkbox.svelte";
    import { getContext } from "svelte";
    import { toastStore } from "../../../stores/toast";
    import ReAuthenticate from "./ReAuthenticate.svelte";
    import type { DelegationChain, ECDSAKeyIdentity } from "@icp-sdk/core/identity";

    const client = getContext<OpenChat>("client");

    interface Props {
        deleting: boolean;
        onClose: () => void;
    }

    let { deleting = $bindable(), onClose }: Props = $props();

    type Step =
        | "intro"
        | "confirm"
        | "preparing"
        | "reveal"
        | "authenticating"
        | "deleting"
        | "polling"
        | "delayed"
        | "done"
        | "error";

    let step = $state<Step>("intro");
    let confirmed = $state(false);
    let revealAck = $state(false);
    let receiptId = $state("");
    let revealWireJson = $state("");
    let localUserIndex = $state("");
    let bearerUrl = $state("");
    let errorMessage = $state("");
    let pollStatus = $state("");
    let resumed = $state(false);

    $effect(() => {
        if (resumed || step !== "intro") return;
        const session = client.loadPersistedCvdrReceiptSession(currentUserIdStore.value);
        if (!session) return;
        resumed = true;
        receiptId = session.receiptId;
        localUserIndex = session.localUserIndex;
        bearerUrl = client.cvdrDownloadUrl(localUserIndex, receiptId);
        if (cvdrSessionAwaitingDelivery(session)) {
            // Delete already succeeded — resume delivery poll only.
            step = "polling";
            void pollUntilAvailable(true);
        } else {
            // Prepare-only: do not poll (no CVDR until delete). Continue to re-auth.
            revealAck = true;
            step = "authenticating";
        }
    });

    async function runPrepare() {
        step = "preparing";
        errorMessage = "";
        const resp = await client.prepareAccountDeletion();
        if (resp.kind !== "success") {
            errorMessage =
                resp.kind === "already_committed"
                    ? "Deletion already started for this account."
                    : resp.kind === "user_canister_unavailable"
                      ? resp.detail
                      : resp.message;
            step = "error";
            return;
        }
        receiptId = resp.receiptId;
        revealWireJson = resp.revealWireJson;
        localUserIndex = resp.localUserIndex;
        bearerUrl = client.cvdrDownloadUrl(localUserIndex, receiptId);
        const persisted = client.persistCvdrReceiptSession(currentUserIdStore.value, {
            receiptId,
            localUserIndex,
            deletionStarted: false,
        });
        if (!persisted) {
            errorMessage = interpolate($_, i18nKey("danger.cvdr.persistFailed"));
            step = "error";
            return;
        }
        revealAck = false;
        step = "reveal";
    }

    function downloadReveal() {
        downloadBytesAsFile(
            new TextEncoder().encode(revealWireJson),
            `openchatzd-reveal-${receiptId.slice(0, 8)}.json`,
        );
    }

    async function copyBearer() {
        try {
            await navigator.clipboard.writeText(bearerUrl);
            toastStore.showSuccessToast(i18nKey("danger.cvdr.copied"));
        } catch {
            toastStore.showFailureToast(i18nKey("danger.cvdr.copyFailed"));
        }
    }

    async function deleteAccount(detail: {
        key: ECDSAKeyIdentity;
        delegation: DelegationChain;
        provider: AuthProvider;
    }) {
        deleting = true;
        step = "deleting";
        // Stef A: persist recovery flag *before* irreversible delete (with readback).
        // No post-delete write may be load-bearing for anonymous recovery.
        if (!client.markCvdrDeletionStarted(currentUserIdStore.value)) {
            step = "error";
            errorMessage = interpolate($_, i18nKey("danger.cvdr.persistFailed"));
            deleting = false;
            return;
        }
        try {
            const success = await client.deleteCurrentUser(
                detail.key.getKeyPair(),
                detail.delegation.toJSON(),
                { deferLogout: true },
            );
            if (!success) {
                // Delete did not commit — roll flag back so prepare-only recovery stays off.
                client.clearCvdrDeletionStarted(currentUserIdStore.value);
                toastStore.showFailureToast(i18nKey("danger.deleteAccountFailed"));
                step = "error";
                errorMessage = "Delete failed";
                return;
            }
            step = "polling";
            await pollUntilAvailable(true);
        } finally {
            deleting = false;
        }
    }

    async function pollUntilAvailable(logoutWhenDone: boolean) {
        if (!localUserIndex || !receiptId) {
            const session = client.loadPersistedCvdrReceiptSession(currentUserIdStore.value);
            if (session) {
                receiptId = session.receiptId;
                localUserIndex = session.localUserIndex;
            }
        }
        const lui = localUserIndex;
        if (!lui || !receiptId) {
            step = "error";
            errorMessage = "Missing receipt session — reopen delete after prepare.";
            return;
        }
        const result = await client.pollCvdrDelivery(lui, receiptId, {
            onStatus: (s) => (pollStatus = s),
        });
        if (result.kind === "available") {
            downloadBytesAsFile(
                result.body,
                `openchatzd-cvdr-${receiptId.slice(0, 8)}.json`,
                result.contentType,
            );
            client.clearPersistedCvdrReceiptSession(currentUserIdStore.value);
            step = "done";
            if (logoutWhenDone) {
                await client.finishDeleteAccountLogout();
            }
            return;
        }
        // Exhaustion is delayed/pending — never success, never logout here.
        step = "delayed";
    }

    /** Explicit handoff to anonymous App recovery while keeping the pending capability. */
    async function handoffToAnonymousRecovery() {
        await client.finishDeleteAccountLogout();
        onClose();
    }
</script>

<Overlay>
    <ModalContent>
        {#snippet header()}
            <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
        {/snippet}
        {#snippet body()}
            {#if step === "intro"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.intro"))} />
            {:else if step === "confirm"}
                <Markdown
                    inline={false}
                    text={interpolate($_, i18nKey("danger.deleteAccountConfirm"))} />
                <div class="ack">
                    <Checkbox
                        id="cvdr-confirm"
                        label={i18nKey("danger.cvdr.confirmCheckbox")}
                        bind:checked={confirmed} />
                </div>
            {:else if step === "preparing"}
                <Translatable resourceKey={i18nKey("danger.cvdr.preparing")} />
            {:else if step === "reveal"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.revealHelp"))} />
                <p class="mono">receipt: {receiptId}</p>
                <p class="warn"><Translatable resourceKey={i18nKey("danger.cvdr.secretWarning")} /></p>
                <p class="warn"><Translatable resourceKey={i18nKey("danger.cvdr.abandonWarning")} /></p>
                <ButtonGroup align="start">
                    <Button small onClick={downloadReveal}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.downloadReveal")} />
                    </Button>
                    <Button small secondary onClick={copyBearer}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.copyLink")} />
                    </Button>
                </ButtonGroup>
                <div class="ack">
                    <Checkbox
                        id="cvdr-reveal-ack"
                        label={i18nKey("danger.cvdr.revealAck")}
                        bind:checked={revealAck} />
                </div>
            {:else if step === "authenticating"}
                <ReAuthenticate onSuccess={deleteAccount} message={i18nKey("danger.reauth")} />
            {:else if step === "deleting"}
                <Translatable resourceKey={i18nKey("danger.deleting")} />
            {:else if step === "polling"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.polling"))} />
                {#if pollStatus}<p class="mono">{pollStatus}</p>{/if}
            {:else if step === "delayed"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.delayed"))} />
                {#if pollStatus}<p class="mono">{pollStatus}</p>{/if}
            {:else if step === "done"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.done"))} />
            {:else if step === "error"}
                <p>{errorMessage}</p>
            {/if}
        {/snippet}
        {#snippet footer()}
            <ButtonGroup>
                {#if step !== "done" && step !== "deleting" && step !== "preparing" && step !== "polling" && step !== "delayed"}
                    <Button small onClick={onClose} secondary>
                        <Translatable resourceKey={i18nKey("cancel")} />
                    </Button>
                {/if}
                {#if step === "intro"}
                    <Button small danger onClick={() => (step = "confirm")}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.continue")} />
                    </Button>
                {:else if step === "confirm"}
                    <Button small secondary onClick={() => (step = "intro")}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.back")} />
                    </Button>
                    <Button small danger disabled={!confirmed} onClick={runPrepare}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.prepare")} />
                    </Button>
                {:else if step === "reveal"}
                    <Button
                        small
                        danger
                        disabled={!revealAck}
                        onClick={() => (step = "authenticating")}>
                        <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
                    </Button>
                {:else if step === "error"}
                    <Button small onClick={() => (step = "intro")}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.retry")} />
                    </Button>
                {:else if step === "delayed"}
                    <Button small secondary onClick={() => void pollUntilAvailable(true)}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.retryPoll")} />
                    </Button>
                    <Button small onClick={() => void handoffToAnonymousRecovery()}>
                        <Translatable resourceKey={i18nKey("danger.cvdr.handoffAnonymous")} />
                    </Button>
                {:else if step === "done"}
                    <Button small onClick={onClose}>
                        <Translatable resourceKey={i18nKey("close")} />
                    </Button>
                {/if}
            </ButtonGroup>
        {/snippet}
    </ModalContent>
</Overlay>

<style>
    .ack {
        margin-top: 1rem;
    }
    .mono {
        font-family: ui-monospace, monospace;
        font-size: 0.8rem;
        word-break: break-all;
        margin: 0.75rem 0;
    }
    .warn {
        color: var(--error, #b00020);
        font-weight: 600;
        margin: 0.5rem 0 1rem;
    }
</style>
