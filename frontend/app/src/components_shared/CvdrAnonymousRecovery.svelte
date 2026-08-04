<script lang="ts">
    import {
        downloadBytesAsFile,
        i18nKey,
        identityStateStore,
        OpenChat,
    } from "openchat-client";
    import { getContext } from "svelte";
    import { _ } from "svelte-i18n";
    import { interpolate } from "../i18n/i18n";
    import Markdown from "./Markdown.svelte";

    const client = getContext<OpenChat>("client");

    type Phase = "idle" | "polling" | "delayed" | "done";

    let phase = $state<Phase>("idle");
    let pollStatus = $state("");
    let receiptId = $state("");
    let started = $state(false);

    // Fresh anonymous session only — account is gone; do not race authenticated prepare/reveal.
    $effect(() => {
        if ($identityStateStore.kind !== "anon") return;
        if (started || phase !== "idle") return;
        const session = client.loadPersistedCvdrReceiptSession(undefined);
        if (!session) return;
        started = true;
        receiptId = session.receiptId;
        phase = "polling";
        void runRecovery(session.localUserIndex, session.receiptId);
    });

    async function runRecovery(localUserIndex: string, rid: string) {
        const result = await client.pollCvdrDelivery(localUserIndex, rid, {
            onStatus: (s) => (pollStatus = s),
        });
        if (result.kind === "available") {
            downloadBytesAsFile(
                result.body,
                `openchatzd-cvdr-${rid.slice(0, 8)}.json`,
                result.contentType,
            );
            client.clearPersistedCvdrReceiptSession(undefined);
            phase = "done";
            return;
        }
        phase = "delayed";
    }

    function dismiss() {
        phase = "idle";
    }
</script>

{#if phase !== "idle"}
    <div class="cvdr-recovery" role="status">
        <div class="panel">
            {#if phase === "polling"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.polling"))} />
                {#if pollStatus}<p class="mono">{pollStatus}</p>{/if}
            {:else if phase === "delayed"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.delayed"))} />
                <p class="mono">receipt: {receiptId}</p>
                <button type="button" class="btn" onclick={dismiss}>
                    {interpolate($_, i18nKey("close"))}
                </button>
            {:else if phase === "done"}
                <Markdown inline={false} text={interpolate($_, i18nKey("danger.cvdr.done"))} />
                <button type="button" class="btn" onclick={dismiss}>
                    {interpolate($_, i18nKey("close"))}
                </button>
            {/if}
        </div>
    </div>
{/if}

<style>
    .cvdr-recovery {
        position: fixed;
        inset: 0;
        z-index: 10000;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(0, 0, 0, 0.45);
        padding: 1.5rem;
    }
    .panel {
        max-width: 28rem;
        width: 100%;
        background: var(--bg, #fff);
        color: var(--txt, #111);
        padding: 1.25rem 1.5rem;
        border-radius: 0.5rem;
        box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
    }
    .mono {
        font-family: ui-monospace, monospace;
        font-size: 0.8rem;
        word-break: break-all;
        margin: 0.75rem 0;
    }
    .btn {
        margin-top: 1rem;
        padding: 0.5rem 1rem;
        cursor: pointer;
    }
</style>
