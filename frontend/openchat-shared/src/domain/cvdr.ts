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

/** Persisted so refresh recovery can poll `/cvdr` after logout without LUI in session. */
export type CvdrReceiptSession = {
    receiptId: string;
    localUserIndex: string;
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
    });
}

/** Accepts legacy bare hex receipt_id strings and JSON sessions. */
export function parseCvdrReceiptSession(raw: string | null | undefined): CvdrReceiptSession | undefined {
    if (raw == null || raw === "") return undefined;
    const trimmed = raw.trim();
    if (trimmed.startsWith("{")) {
        try {
            const v = JSON.parse(trimmed) as Partial<CvdrReceiptSession>;
            if (
                typeof v.receiptId === "string" &&
                v.receiptId.length === 64 &&
                typeof v.localUserIndex === "string" &&
                v.localUserIndex.length > 0
            ) {
                return {
                    receiptId: v.receiptId.toLowerCase(),
                    localUserIndex: v.localUserIndex,
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
