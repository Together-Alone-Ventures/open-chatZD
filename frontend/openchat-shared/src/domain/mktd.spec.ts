import {
    mktdCorridorStatus,
    mktdIsFinalized,
    mktdIsTombstonedPending,
    type MktdDeletionState,
} from "./mktd";

const notStarted: MktdDeletionState = { tombstoned: false, pending: false };
const tombstonedPending: MktdDeletionState = { tombstoned: true, pending: true };
const finalized: MktdDeletionState = { tombstoned: true, pending: false };

describe("mktd deletion state helpers", () => {
    test("mktdIsFinalized only when tombstoned and not pending", () => {
        expect(mktdIsFinalized(finalized)).toBe(true);
        expect(mktdIsFinalized(tombstonedPending)).toBe(false);
        expect(mktdIsFinalized(notStarted)).toBe(false);
    });

    test("mktdIsTombstonedPending only when tombstoned and pending", () => {
        expect(mktdIsTombstonedPending(tombstonedPending)).toBe(true);
        expect(mktdIsTombstonedPending(finalized)).toBe(false);
        expect(mktdIsTombstonedPending(notStarted)).toBe(false);
    });
});

describe("mktdCorridorStatus — shell guard fail-closed distinction (G ruling b)", () => {
    test("confirmed tombstoned-pending blocks", () => {
        expect(mktdCorridorStatus(tombstonedPending)).toBe("blocked");
    });

    test("confirmed tombstoned-finalized blocks", () => {
        expect(mktdCorridorStatus(finalized)).toBe("blocked");
    });

    test("confirmed not-tombstoned is clear (normal app)", () => {
        expect(mktdCorridorStatus(notStarted)).toBe("clear");
    });

    test("unknown state from a failed read does NOT trap a normal user", () => {
        // The one place the shell guard could wrongly lock out non-deleting users:
        // a transient read error (undefined) must be "unknown", never "blocked".
        expect(mktdCorridorStatus(undefined)).toBe("unknown");
    });
});
