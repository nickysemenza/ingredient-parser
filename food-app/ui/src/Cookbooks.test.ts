import { describe, expect, it } from "vitest";
import { auditStatus } from "./Cookbooks";

describe("auditStatus", () => {
  const failed = { source_sha256: "book", candidate_revision: 1n, group: 0, findings: [{ kind: "missing", message: "omitted", spans: [] }], corrections: [], accepted: false, reaudited: false };
  const passed = { source_sha256: "book", candidate_revision: 1n, group: 1, findings: [], corrections: [], accepted: true, reaudited: false };

  it("uses run completion instead of the last audit group", () => {
    expect(
      auditStatus([failed, passed], "complete", false),
    ).toBe("accepted");
    expect(auditStatus([passed], "incomplete", true)).toBe("findings");
    expect(auditStatus([failed, passed], "incomplete", true)).toBe("findings");
  });

  it("does not claim assessment when no audit exists", () => {
    expect(auditStatus([], "complete", false)).toBe("not assessed");
  });

  it("keeps a correction re-audit accepted when the run completed", () => {
    expect(
      auditStatus([{ ...failed, reaudited: true, accepted: false }, passed], "complete", false),
    ).toBe("accepted");
  });
});
