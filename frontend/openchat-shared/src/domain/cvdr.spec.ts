import { describe, expect, it, vi } from "vitest";
import {
    cvdrDownloadUrl,
    cvdrReceiptStorageKey,
    cvdrSessionAwaitingDelivery,
    parseCvdrReceiptSession,
    pollCvdrHttp,
    pollCvdrUntilSettled,
    serializeCvdrReceiptSession,
} from "./cvdr";

describe("cvdr helpers", () => {
    it("builds raw /cvdr path", () => {
        const url = cvdrDownloadUrl(
            "https://{canisterId}.icp0.io",
            "aaaaa-aa",
            "AB".repeat(32),
        );
        expect(url).toBe(`https://aaaaa-aa.raw.icp0.io/cvdr/${"ab".repeat(32)}`);
    });

    it("leaves already-raw hosts unchanged", () => {
        const url = cvdrDownloadUrl(
            "https://{canisterId}.raw.icp0.io/",
            "bbbbb-bb",
            "cd".repeat(32),
        );
        expect(url).toBe(`https://bbbbb-bb.raw.icp0.io/cvdr/${"cd".repeat(32)}`);
    });

    it("storage key is per-user", () => {
        expect(cvdrReceiptStorageKey("user-1")).toBe("oc_cvdr_receipt_user-1");
    });

    it("round-trips receipt session JSON with lowercase receipt id", () => {
        const raw = serializeCvdrReceiptSession({
            receiptId: "AB".repeat(32),
            localUserIndex: "aaaaa-aa",
        });
        expect(parseCvdrReceiptSession(raw)).toEqual({
            receiptId: "ab".repeat(32),
            localUserIndex: "aaaaa-aa",
            deletionStarted: false,
        });
    });

    it("round-trips deletionStarted flag for post-delete recovery", () => {
        const raw = serializeCvdrReceiptSession({
            receiptId: "ab".repeat(32),
            localUserIndex: "aaaaa-aa",
            deletionStarted: true,
        });
        const parsed = parseCvdrReceiptSession(raw);
        expect(parsed?.deletionStarted).toBe(true);
        expect(cvdrSessionAwaitingDelivery(parsed!)).toBe(true);
    });

    it("prepare-only session is not awaiting delivery", () => {
        expect(
            cvdrSessionAwaitingDelivery({
                receiptId: "ab".repeat(32),
                localUserIndex: "aaaaa-aa",
                deletionStarted: false,
            }),
        ).toBe(false);
        expect(
            cvdrSessionAwaitingDelivery({
                receiptId: "ab".repeat(32),
                localUserIndex: "aaaaa-aa",
            }),
        ).toBe(false);
    });

    it("rejects legacy bare receipt hex (missing LUI — cannot resume poll)", () => {
        expect(parseCvdrReceiptSession("ab".repeat(32))).toBeUndefined();
    });

    it("rejects non-hex 64-char receiptId", () => {
        expect(
            parseCvdrReceiptSession(
                JSON.stringify({
                    receiptId: "z".repeat(64),
                    localUserIndex: "aaaaa-aa",
                }),
            ),
        ).toBeUndefined();
    });

    it("rejects malformed session JSON", () => {
        expect(parseCvdrReceiptSession('{"receiptId":"short"}')).toBeUndefined();
        expect(parseCvdrReceiptSession("")).toBeUndefined();
        expect(parseCvdrReceiptSession(null)).toBeUndefined();
    });
});

describe("pollCvdrHttp", () => {
    it("maps 200 to available", async () => {
        const fetchImpl = vi.fn(async () =>
            new Response(new Uint8Array([1, 2, 3]), {
                status: 200,
                headers: { "content-type": "application/json" },
            }),
        ) as unknown as typeof fetch;
        const status = await pollCvdrHttp("https://example/cvdr/x", fetchImpl);
        expect(status.kind).toBe("available");
        if (status.kind === "available") {
            expect([...status.body]).toEqual([1, 2, 3]);
        }
    });

    it("maps 202 pending with retry_after_secs", async () => {
        const fetchImpl = vi.fn(async () =>
            new Response(JSON.stringify({ status: "pending", retry_after_secs: 7 }), {
                status: 202,
                headers: { "content-type": "application/json" },
            }),
        ) as unknown as typeof fetch;
        const status = await pollCvdrHttp("https://example/cvdr/x", fetchImpl);
        expect(status).toEqual({ kind: "pending", retryAfterSecs: 7 });
    });

    it("maps 404/400", async () => {
        const notFound = vi.fn(async () => new Response("{}", { status: 404 })) as unknown as typeof fetch;
        expect(await pollCvdrHttp("u", notFound)).toEqual({ kind: "unknown" });
        const bad = vi.fn(async () => new Response("{}", { status: 400 })) as unknown as typeof fetch;
        expect(await pollCvdrHttp("u", bad)).toEqual({ kind: "malformed" });
    });
});

describe("pollCvdrUntilSettled", () => {
    it("returns available without clearing anything", async () => {
        const body = new Uint8Array([9]);
        const result = await pollCvdrUntilSettled(
            async () => ({ kind: "available", body, contentType: "application/json" }),
            { sleep: async () => undefined },
        );
        expect(result).toEqual({ kind: "available", body, contentType: "application/json" });
    });

    it("returns delayed on exhaustion — never available", async () => {
        let calls = 0;
        const result = await pollCvdrUntilSettled(
            async () => {
                calls += 1;
                return { kind: "pending", retryAfterSecs: 1 };
            },
            { maxAttempts: 3, softWaitUnknownAttempts: 0, sleep: async () => undefined },
        );
        expect(result).toEqual({ kind: "delayed" });
        expect(calls).toBe(3);
    });

    it("soft-waits unknown then continues", async () => {
        const statuses = [
            { kind: "unknown" as const },
            { kind: "unknown" as const },
            {
                kind: "available" as const,
                body: new Uint8Array([1]),
                contentType: "application/json",
            },
        ];
        let i = 0;
        const result = await pollCvdrUntilSettled(
            async () => statuses[i++]!,
            { maxAttempts: 5, softWaitUnknownAttempts: 5, sleep: async () => undefined },
        );
        expect(result.kind).toBe("available");
        expect(i).toBe(3);
    });
});
