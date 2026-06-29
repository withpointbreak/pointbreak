import { describe, expect, it } from "vitest";
import { escapeHtml } from "../src/escape";

describe("escapeHtml", () => {
  it("maps each HTML-significant character to its entity", () => {
    expect(escapeHtml("&")).toBe("&amp;");
    expect(escapeHtml("<")).toBe("&lt;");
    expect(escapeHtml(">")).toBe("&gt;");
    expect(escapeHtml('"')).toBe("&quot;");
    expect(escapeHtml("'")).toBe("&#39;");
  });

  it("escapes every dangerous character in mixed content (XSS-significant)", () => {
    expect(escapeHtml("<script>alert(\"x\" & 'y')</script>")).toBe(
      "&lt;script&gt;alert(&quot;x&quot; &amp; &#39;y&#39;)&lt;/script&gt;",
    );
  });

  it("leaves safe text and unicode untouched", () => {
    expect(escapeHtml("plain text 123")).toBe("plain text 123");
    expect(escapeHtml("café ☕ 你好")).toBe("café ☕ 你好");
    expect(escapeHtml("")).toBe("");
  });

  it("coerces non-string input via String() (legacy behaviour)", () => {
    expect(escapeHtml(42)).toBe("42");
    expect(escapeHtml(null)).toBe("null");
    expect(escapeHtml(undefined)).toBe("undefined");
  });
});
