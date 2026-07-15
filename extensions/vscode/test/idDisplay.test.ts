import { describe, expect, it } from "vitest";
import { shortReferenceId } from "../src/idDisplay";

describe("shortReferenceId", () => {
  it("uses the final typed-ID component and preserves short values", () => {
    expect(shortReferenceId("rev:sha256:1234567890abcdef")).toBe(
      "1234567890ab",
    );
    expect(shortReferenceId("assessment:short")).toBe("short");
  });
});
