import { describe, expect, it, vi } from "vitest";
import {
    cvdrDownloadUrl,
    cvdrReceiptStorageKey,
    parseCvdrReceiptSession,
    pollCvdrHttp,
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
        });
    });

    it("rejects legacy bare receipt hex (missing LUI — cannot resume poll)", () => {
        expect(parseCvdrReceiptSession("ab".repeat(32))).toBeUndefined();
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
