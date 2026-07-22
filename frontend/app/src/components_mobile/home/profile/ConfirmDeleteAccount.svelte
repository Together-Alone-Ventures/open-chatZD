<script lang="ts">
    import type { DelegationChain, ECDSAKeyIdentity } from "@icp-sdk/core/identity";
    import { Column, CommonButton, Container, Sheet, Title } from "component-lib";
    import { AuthProvider, i18nKey, OpenChat } from "openchat-client";
    import { getContext, onMount } from "svelte";
    import { _ } from "svelte-i18n";
    import Delete from "svelte-material-icons/DeleteForeverOutline.svelte";
    import { interpolate } from "../../../i18n/i18n";
    import { toastStore } from "../../../stores/toast";
    import Translatable from "../../Translatable.svelte";
    import Markdown from "@shared_components/Markdown.svelte";
    import ReAuthenticate from "./ReAuthenticate.svelte";

    const client = getContext<OpenChat>("client");

    interface Props {
        deleting: boolean;
        onClose: () => void;
        authenticating?: boolean;
    }

    let { deleting = $bindable(), onClose, authenticating = $bindable(false) }: Props = $props();

    // Same A→B→C → receipt → guarded teardown flow as the desktop component, in the
    // mobile shell. `authenticating` is kept for parity with the previous API.
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
    let downloading = $state(false);

    // The parent mounts this component by setting `authenticating = true`; we keep it
    // true for the whole multi-step flow (our own `step` drives the UI) and let the
    // parent's `onClose` reset it when the flow finishes or is cancelled.
    void authenticating;

    onMount(async () => {
        try {
            const state = await client.mktdPendingDeletionState();
            if (state.tombstoned && !state.pending) {
                receiptIdHex = client.mktdStoredReceiptIdHex();
                step = "receipt";
            } else if (state.tombstoned && state.pending) {
                await runFlow();
            } else {
                step = "intro";
            }
        } catch {
            step = "intro";
        }
    });

    async function runFlow() {
        step = "processing";
        const res = await client.mktdRunDeletionReceiptFlow();
        if (res.kind === "success") {
            receiptIdHex = res.receiptIdHex;
            step = "receipt";
        } else {
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
            void finishDeletion();
        } else {
            void runFlow();
        }
    }

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

    async function finishDeletion() {
        if (authKey === undefined || authDelegation === undefined) {
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

<Sheet>
    <Column gap={"xl"} padding={"xl"}>
        <Title fontWeight={"bold"}>
            {#if step === "receipt" || step === "deleting"}
                <Translatable resourceKey={i18nKey("danger.mktd.receiptTitle")} />
            {:else if step === "confirm"}
                <Translatable resourceKey={i18nKey("danger.mktd.finalConfirmTitle")} />
            {:else}
                <Translatable resourceKey={i18nKey("danger.mktd.introTitle")} />
            {/if}
        </Title>

        {#if step === "recovering" || step === "processing" || step === "deleting"}
            <p class="processing">
                <Translatable resourceKey={i18nKey("danger.mktd.processing")} />
            </p>
        {:else if step === "intro"}
            <Markdown inline={false} text={interpolate($_, i18nKey("danger.mktd.intro"))} />
        {:else if step === "confirm"}
            <p class="final-confirm">
                <Translatable resourceKey={i18nKey("danger.mktd.finalConfirm")} />
            </p>
            <label class="confirm-check">
                <input type="checkbox" bind:checked={confirmed} />
                <Translatable resourceKey={i18nKey("danger.mktd.confirmCheckbox")} />
            </label>
        {:else if step === "reauth"}
            <ReAuthenticate onSuccess={onReauthSuccess} message={i18nKey("danger.reauth")} />
        {:else if step === "error"}
            <p class="error"><Translatable resourceKey={i18nKey("danger.mktd.flowError")} /></p>
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

        <Container gap={"md"} mainAxisAlignment={"end"} crossAxisAlignment={"end"}>
            {#if step === "intro"}
                <CommonButton mode={"default"} onClick={onClose} size={"small_text"}>
                    <Translatable resourceKey={i18nKey("cancel")} />
                </CommonButton>
                <CommonButton mode={"active"} onClick={() => (step = "confirm")} size={"medium"}>
                    <Translatable resourceKey={i18nKey("danger.mktd.continue")} />
                </CommonButton>
            {:else if step === "confirm"}
                <CommonButton mode={"default"} onClick={() => (step = "intro")} size={"small_text"}>
                    <Translatable resourceKey={i18nKey("danger.mktd.back")} />
                </CommonButton>
                <CommonButton
                    mode={"active"}
                    disabled={!confirmed}
                    onClick={() => (step = "reauth")}
                    size={"medium"}>
                    {#snippet icon(color, size)}
                        <Delete {color} {size}></Delete>
                    {/snippet}
                    <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
                </CommonButton>
            {:else if step === "error"}
                <CommonButton mode={"default"} onClick={onClose} size={"small_text"}>
                    <Translatable resourceKey={i18nKey("cancel")} />
                </CommonButton>
                <CommonButton mode={"active"} onClick={runFlow} size={"medium"}>
                    <Translatable resourceKey={i18nKey("danger.mktd.retry")} />
                </CommonButton>
            {:else if step === "receipt"}
                <CommonButton
                    mode={"default"}
                    disabled={receiptIdHex === undefined || downloading}
                    loading={downloading}
                    onClick={downloadReceipt}
                    size={"small_text"}>
                    <Translatable
                        resourceKey={i18nKey(
                            downloading ? "danger.mktd.downloading" : "danger.mktd.download",
                        )} />
                </CommonButton>
                <CommonButton
                    mode={"default"}
                    disabled={receiptIdHex === undefined}
                    onClick={copyReceiptId}
                    size={"small_text"}>
                    <Translatable resourceKey={i18nKey("danger.mktd.copyId")} />
                </CommonButton>
                <CommonButton
                    mode={"active"}
                    disabled={deleting}
                    loading={deleting}
                    onClick={finishDeletion}
                    size={"medium"}>
                    {#snippet icon(color, size)}
                        <Delete {color} {size}></Delete>
                    {/snippet}
                    <Translatable
                        resourceKey={i18nKey(
                            deleting ? "danger.mktd.finishing" : "danger.mktd.finish",
                        )} />
                </CommonButton>
            {/if}
        </Container>
    </Column>
</Sheet>

<style lang="scss">
    .underway,
    .error {
        font-weight: 700;
    }
    .confirm-check {
        display: flex;
        gap: $sp3;
        align-items: center;
    }
    .receipt-id {
        display: flex;
        flex-direction: column;
        gap: $sp2;
        .value {
            word-break: break-all;
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
</style>
