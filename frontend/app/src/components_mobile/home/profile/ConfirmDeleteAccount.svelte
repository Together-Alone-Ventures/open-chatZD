<script lang="ts">
    import type { DelegationChain, ECDSAKeyIdentity } from "@icp-sdk/core/identity";
    import { Column, CommonButton, Container, Sheet, Title } from "component-lib";
    import {
        AuthProvider,
        currentUserIdStore,
        downloadBytesAsFile,
        i18nKey,
        OpenChat,
        type CvdrPollStatus,
    } from "openchat-client";
    import { getContext } from "svelte";
    import { _ } from "svelte-i18n";
    import Delete from "svelte-material-icons/DeleteForeverOutline.svelte";
    import { interpolate } from "../../../i18n/i18n";
    import { toastStore } from "../../../stores/toast";
    import Translatable from "../../Translatable.svelte";
    import Checkbox from "../../Checkbox.svelte";
    import Markdown from "@shared_components/Markdown.svelte";
    import ReAuthenticate from "./ReAuthenticate.svelte";

    const client = getContext<OpenChat>("client");

    interface Props {
        deleting: boolean;
        onClose: () => void;
        authenticating?: boolean;
    }

    let { deleting = $bindable(), onClose, authenticating = $bindable(false) }: Props = $props();

    type Step = "confirm" | "preparing" | "reveal" | "deleting" | "polling" | "done" | "error";

    let step = $state<Step>("confirm");
    let confirmed = $state(false);
    let revealAck = $state(false);
    let receiptId = $state("");
    let revealWireJson = $state("");
    let localUserIndex = $state("");
    let bearerUrl = $state("");
    let errorMessage = $state("");
    let pollStatus = $state("");

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
        client.persistCvdrReceiptSession(currentUserIdStore.value, {
            receiptId,
            localUserIndex,
        });
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

    function deleteAccount(detail: {
        key: ECDSAKeyIdentity;
        delegation: DelegationChain;
        provider: AuthProvider;
    }) {
        deleting = true;
        authenticating = false;
        step = "deleting";
        return client
            .deleteCurrentUser(detail.key.getKeyPair(), detail.delegation.toJSON(), {
                deferLogout: true,
            })
            .then(async (success) => {
                if (!success) {
                    toastStore.showFailureToast(i18nKey("danger.deleteAccountFailed"));
                    step = "error";
                    errorMessage = "Delete failed";
                    return;
                }
                step = "polling";
                await pollUntilAvailable(true);
            })
            .finally(() => (deleting = false));
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
        for (let i = 0; i < 60; i++) {
            const status: CvdrPollStatus = await client.pollCvdr(lui, receiptId);
            if (status.kind === "available") {
                downloadBytesAsFile(
                    status.body,
                    `openchatzd-cvdr-${receiptId.slice(0, 8)}.json`,
                    status.contentType,
                );
                client.clearPersistedCvdrReceiptSession(currentUserIdStore.value);
                step = "done";
                if (logoutWhenDone) {
                    await client.finishDeleteAccountLogout();
                }
                return;
            }
            if (status.kind === "pending") {
                pollStatus = `Receipt pending… retry in ${status.retryAfterSecs}s`;
                await new Promise((r) => setTimeout(r, Math.max(1, status.retryAfterSecs) * 1000));
                continue;
            }
            pollStatus = status.kind === "error" ? status.detail : status.kind;
            await new Promise((r) => setTimeout(r, 3000));
        }
        step = "done";
        if (logoutWhenDone) {
            await client.finishDeleteAccountLogout();
        }
    }
</script>

<Sheet>
    <Column gap={"xl"} padding={"xl"}>
        <Title fontWeight={"bold"}>
            <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
        </Title>

        {#if authenticating}
            <ReAuthenticate onSuccess={deleteAccount} message={i18nKey("danger.reauth")} />
        {:else if step === "confirm"}
            <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.intro"))} />
            <Markdown
                inline={false}
                text={interpolate($_, i18nKey("danger.deleteAccountConfirm"))} />
            <Checkbox
                id="cvdr-confirm-m"
                label={i18nKey("danger.cvdr.confirmCheckbox")}
                bind:checked={confirmed} />
        {:else if step === "preparing"}
            <Translatable resourceKey={i18nKey("danger.cvdr.preparing")} />
        {:else if step === "reveal"}
            <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.revealHelp"))} />
            <p class="mono">receipt: {receiptId}</p>
            <p class="warn"><Translatable resourceKey={i18nKey("danger.cvdr.secretWarning")} /></p>
            <Container gap={"md"}>
                <CommonButton mode={"default"} size={"small_text"} onClick={downloadReveal}>
                    <Translatable resourceKey={i18nKey("danger.cvdr.downloadReveal")} />
                </CommonButton>
                <CommonButton mode={"default"} size={"small_text"} onClick={copyBearer}>
                    <Translatable resourceKey={i18nKey("danger.cvdr.copyLink")} />
                </CommonButton>
            </Container>
            <Checkbox
                id="cvdr-reveal-ack-m"
                label={i18nKey("danger.cvdr.revealAck")}
                bind:checked={revealAck} />
        {:else if step === "deleting"}
            <Translatable resourceKey={i18nKey("danger.deleting")} />
        {:else if step === "polling"}
            <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.polling"))} />
            {#if pollStatus}<p class="mono">{pollStatus}</p>{/if}
        {:else if step === "done"}
            <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.done"))} />
        {:else if step === "error"}
            <p>{errorMessage}</p>
        {/if}

        <Container gap={"md"} mainAxisAlignment={"end"} crossAxisAlignment={"end"}>
            {#if step !== "done" && step !== "deleting" && step !== "preparing" && step !== "polling" && !authenticating}
                <CommonButton mode={"default"} onClick={onClose} size={"small_text"}>
                    <Translatable resourceKey={i18nKey("cancel")}></Translatable>
                </CommonButton>
            {/if}
            {#if step === "confirm" && !authenticating}
                <CommonButton
                    mode={"active"}
                    disabled={!confirmed || deleting}
                    onClick={runPrepare}
                    size={"medium"}>
                    <Translatable resourceKey={i18nKey("danger.cvdr.prepare")} />
                </CommonButton>
            {:else if step === "reveal" && !authenticating}
                <CommonButton
                    mode={"active"}
                    disabled={!revealAck || deleting}
                    onClick={() => (authenticating = true)}
                    size={"medium"}>
                    {#snippet icon(color, size)}
                        <Delete {color} {size}></Delete>
                    {/snippet}
                    <Translatable resourceKey={i18nKey("danger.deleteAccount")} />
                </CommonButton>
            {:else if step === "error"}
                <CommonButton mode={"active"} onClick={() => (step = "confirm")} size={"medium"}>
                    <Translatable resourceKey={i18nKey("danger.cvdr.retry")} />
                </CommonButton>
            {:else if step === "done"}
                <CommonButton mode={"active"} onClick={onClose} size={"medium"}>
                    <Translatable resourceKey={i18nKey("close")} />
                </CommonButton>
            {/if}
        </Container>
    </Column>
</Sheet>

<style>
    .mono {
        font-family: ui-monospace, monospace;
        font-size: 0.8rem;
        word-break: break-all;
    }
    .warn {
        color: var(--error, #b00020);
        font-weight: 600;
    }
</style>
