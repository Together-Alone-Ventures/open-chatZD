/** OpenChatZD CVDR delivery-leg types (spec §11) — frontend V1. */

export type PrepareAccountDeletionSuccess = {
    kind: "success";
    receiptId: string;
    revealWireJson: string;
    localUserIndex: string;
};

export type PrepareAccountDeletionResponse =
    | PrepareAccountDeletionSuccess
    | { kind: "already_committed" }
    | { kind: "user_canister_unavailable"; detail: string }
    | { kind: "error"; message: string };

export type CvdrPollStatus =
    | { kind: "available"; body: Uint8Array; contentType: string }
    | { kind: "pending"; retryAfterSecs: number }
    | { kind: "unknown" }
    | { kind: "malformed" }
    | { kind: "error"; status: number; detail: string };

/**
 * Persisted so refresh recovery can poll `/cvdr` after logout without LUI in session.
 * `deletionStarted` must be written (with readback) *before* the irreversible delete —
 * no post-delete write may be load-bearing for recovery. Prepare-only sessions keep
 * the flag false so auto-poll / anonymous recovery do not run.
 */
export type CvdrReceiptSession = {
    receiptId: string;
    localUserIndex: string;
    deletionStarted?: boolean;
};

export const CVDR_RECEIPT_STORAGE_PREFIX = "oc_cvdr_receipt_";
/** Last in-flight deletion session (survives logout so poll can finish / resume). */
export const CVDR_RECEIPT_PENDING_KEY = "oc_cvdr_receipt_pending";

export function cvdrReceiptStorageKey(userId: string): string {
    return `${CVDR_RECEIPT_STORAGE_PREFIX}${userId}`;
}

export function serializeCvdrReceiptSession(session: CvdrReceiptSession): string {
    return JSON.stringify({
        receiptId: session.receiptId.toLowerCase(),
        localUserIndex: session.localUserIndex,
        deletionStarted: session.deletionStarted === true,
    });
}

/** Accepts legacy bare hex receipt_id strings and JSON sessions. */
export function parseCvdrReceiptSession(raw: string | null | undefined): CvdrReceiptSession | undefined {
    if (raw == null || raw === "") return undefined;
    const trimmed = raw.trim();
    if (trimmed.startsWith("{")) {
        try {
            const v = JSON.parse(trimmed) as Partial<CvdrReceiptSession> & {
                deletionStarted?: unknown;
            };
            if (
                typeof v.receiptId === "string" &&
                /^[0-9a-fA-F]{64}$/.test(v.receiptId) &&
                typeof v.localUserIndex === "string" &&
                v.localUserIndex.length > 0
            ) {
                return {
                    receiptId: v.receiptId.toLowerCase(),
                    localUserIndex: v.localUserIndex,
                    deletionStarted: v.deletionStarted === true,
                };
            }
        } catch {
            return undefined;
        }
        return undefined;
    }
    // Legacy: receipt hex only — cannot poll without LUI.
    if (/^[0-9a-fA-F]{64}$/.test(trimmed)) {
        return undefined;
    }
    return undefined;
}

/**
 * True once the client has committed to delete (flag set before the irreversible
 * call). Safe to auto-poll / anonymous-recover. Prepare-only stays false.
 */
export function cvdrSessionAwaitingDelivery(session: CvdrReceiptSession): boolean {
    return session.deletionStarted === true;
}

/**
 * Stef A recovery ordering: persist `deletionStarted` (with caller-provided
 * readback) *before* the irreversible delete. No post-delete write may be
 * load-bearing for recovery. On delete failure or throw, clear the flag
 * (with retries). If clear still fails after a failed delete, return
 * `delete_failed_flag_stuck` so the UI can refuse to pretend prepare-only.
 */
export async function runDeletionWithPreflightRecoveryFlag(gate: {
    markStarted: () => boolean;
    clearStarted: () => boolean;
    deleteAccount: () => Promise<boolean>;
    clearAttempts?: number;
}): Promise<"persist_failed" | "delete_failed" | "delete_failed_flag_stuck" | "deleted"> {
    const clearAttempts = gate.clearAttempts ?? 3;
    const clearHard = (): boolean => {
        for (let i = 0; i < clearAttempts; i++) {
            if (gate.clearStarted()) return true;
        }
        return false;
    };

    if (!gate.markStarted()) {
        return "persist_failed";
    }
    try {
        const success = await gate.deleteAccount();
        if (!success) {
            return clearHard() ? "delete_failed" : "delete_failed_flag_stuck";
        }
        return "deleted";
    } catch (err) {
        clearHard();
        throw err;
    }
}

/**
 * Poll until Available or attempts exhausted. Does not clear storage or logout.
 * Extracted for unit tests; OpenChat.pollCvdrDelivery delegates here.
 */
export async function pollCvdrUntilSettled(
    pollOnce: () => Promise<CvdrPollStatus>,
    options?: {
        maxAttempts?: number;
        softWaitUnknownAttempts?: number;
        onStatus?: (status: string) => void;
        sleep?: (ms: number) => Promise<void>;
    },
): Promise<
    | { kind: "available"; body: Uint8Array; contentType: string }
    | { kind: "delayed" }
> {
    const maxAttempts = options?.maxAttempts ?? 60;
    const softWaitUnknown = options?.softWaitUnknownAttempts ?? 5;
    const sleep = options?.sleep ?? ((ms: number) => new Promise((r) => setTimeout(r, ms)));
    for (let i = 0; i < maxAttempts; i++) {
        const status = await pollOnce();
        if (status.kind === "available") {
            return {
                kind: "available",
                body: status.body,
                contentType: status.contentType,
            };
        }
        if (status.kind === "pending") {
            options?.onStatus?.(`Receipt pending… retry in ${status.retryAfterSecs}s`);
            await sleep(Math.max(1, status.retryAfterSecs) * 1000);
            continue;
        }
        if (status.kind === "unknown" && i < softWaitUnknown) {
            options?.onStatus?.("Not found yet — waiting for draft…");
            await sleep(2000);
            continue;
        }
        options?.onStatus?.(status.kind === "error" ? status.detail : status.kind);
        await sleep(3000);
    }
    return { kind: "delayed" };
}

export function cvdrDownloadUrl(canisterUrlPath: string, localUserIndex: string, receiptIdHex: string): string {
    const base = canisterUrlPath.replace("{canisterId}", localUserIndex);
    // Prefer raw domain for bearer fetch (spec §11.1).
    const rawBase = base.includes(".raw.")
        ? base.replace(/\/$/, "")
        : base.replace(".icp0.io", ".raw.icp0.io").replace(/\/$/, "");
    return `${rawBase}/cvdr/${receiptIdHex.toLowerCase()}`;
}

export async function pollCvdrHttp(url: string, fetchImpl: typeof fetch = fetch): Promise<CvdrPollStatus> {
    try {
        const res = await fetchImpl(url, { method: "GET", cache: "no-store" });
        if (res.status === 200) {
            const body = new Uint8Array(await res.arrayBuffer());
            const contentType = res.headers.get("content-type") ?? "application/json";
            return { kind: "available", body, contentType };
        }
        if (res.status === 202) {
            let retryAfterSecs = 5;
            try {
                const j = (await res.json()) as { retry_after_secs?: number };
                if (typeof j.retry_after_secs === "number") {
                    retryAfterSecs = j.retry_after_secs;
                }
            } catch {
                /* ignore */
            }
            return { kind: "pending", retryAfterSecs };
        }
        if (res.status === 404) return { kind: "unknown" };
        if (res.status === 400) return { kind: "malformed" };
        return { kind: "error", status: res.status, detail: await res.text() };
    } catch (e) {
        return { kind: "error", status: 0, detail: e instanceof Error ? e.message : String(e) };
    }
}

export function downloadBytesAsFile(bytes: Uint8Array, filename: string, mime = "application/json"): void {
    // Fresh ArrayBuffer: BlobPart rejects ArrayBufferLike / SharedArrayBuffer under current DOM libs.
    const ab = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
    const blob = new Blob([ab], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
}
