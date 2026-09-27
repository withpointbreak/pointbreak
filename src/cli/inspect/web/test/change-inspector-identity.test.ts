import { describe, expect, it } from "vitest";
import { decodeInspectorIdentity } from "../src/change-inspector-identity";

const identity = {
  schema: "pointbreak.inspect-identity",
  storeIdentity: "store:sha256:one",
  contextIdentity: "context:sha256:one",
  repository: "pointbreak",
  placement: { tier: "family", label: "family store" },
  family: { id: "pointbreak" },
  worktree: "feat-change-aware-inspector",
};

describe("Inspector identity DTO", () => {
  it("accepts the closed path-private identity document", () => {
    expect(decodeInspectorIdentity(identity)).toEqual(identity);
  });

  it("accepts an optional server-supplied family-split advisory", () => {
    const advised = {
      ...identity,
      familyLinkAdvisory:
        "a sibling worktree is linked to family store pointbreak; run pointbreak store link pointbreak",
    };
    expect(decodeInspectorIdentity(advised)).toEqual(advised);
    expect(
      decodeInspectorIdentity(identity).familyLinkAdvisory,
    ).toBeUndefined();
  });

  it.each([
    ["empty advisory", { ...identity, familyLinkAdvisory: "" }],
    ["non-string advisory", { ...identity, familyLinkAdvisory: true }],
    ["wrong schema", { ...identity, schema: "pointbreak.inspect-other" }],
    ["empty repository", { ...identity, repository: "" }],
    [
      "unknown placement",
      { ...identity, placement: { tier: "shared", label: "shared store" } },
    ],
    [
      "mismatched placement label",
      { ...identity, placement: { tier: "clone", label: "family store" } },
    ],
    ["invalid family slug", { ...identity, family: { id: "Pointbreak" } }],
    ["overlong family slug", { ...identity, family: { id: "a".repeat(65) } }],
    ["unknown member", { ...identity, absolutePath: "/private/repository" }],
    [
      "Windows repository path",
      { ...identity, repository: "C:\\private\\pointbreak" },
    ],
    [
      "Windows worktree path",
      { ...identity, worktree: "C:\\private\\pointbreak-worktree" },
    ],
  ])("refuses %s", (_label, value) => {
    expect(() => decodeInspectorIdentity(value)).toThrow(
      "invalid Inspector identity DTO",
    );
  });
});
