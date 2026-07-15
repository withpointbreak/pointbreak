import { describe, expect, it } from "vitest";
import {
  renderBodyContent,
  renderContentHtml,
  renderMarkdown,
  renderMarkdownInline,
} from "../src/markdown";

function parse(html: string): Document {
  return new DOMParser().parseFromString(html, "text/html");
}

describe("renderMarkdown", () => {
  it("renders ATX headings at their level", () => {
    expect(renderMarkdown("# Title")).toBe("<h1>Title</h1>");
    expect(renderMarkdown("### Sub")).toBe("<h3>Sub</h3>");
    expect(renderMarkdown("###### Deep")).toBe("<h6>Deep</h6>");
  });

  it("joins wrapped lines into one paragraph and splits on blank lines", () => {
    expect(renderMarkdown("line one\nline two")).toBe(
      "<p>line one line two</p>",
    );
    expect(renderMarkdown("first\n\nsecond")).toBe("<p>first</p><p>second</p>");
  });

  it("renders bold and italic emphasis", () => {
    expect(renderMarkdown("**bold** and *italic*")).toBe(
      "<p><strong>bold</strong> and <em>italic</em></p>",
    );
  });

  it("renders unordered and ordered lists", () => {
    expect(renderMarkdown("- a\n- b")).toBe("<ul><li>a</li><li>b</li></ul>");
    expect(renderMarkdown("1. x\n2. y")).toBe("<ol><li>x</li><li>y</li></ol>");
  });

  it("renders a fenced code block with escaped contents", () => {
    const doc = parse(renderMarkdown("```\ncode <x> & y\n```"));
    const code = doc.querySelector("pre > code");
    expect(code?.textContent).toBe("code <x> & y");
    expect(doc.querySelector("x")).toBeNull();
  });

  it("escapes raw HTML in body text (XSS-safe)", () => {
    const doc = parse(renderMarkdown("<img src=x onerror=alert(1)>"));
    expect(doc.querySelector("img")).toBeNull();
    expect(doc.body.textContent).toContain("<img src=x onerror=alert(1)>");
  });
});

describe("renderMarkdownInline", () => {
  it("renders inline code with escaped contents", () => {
    const code = parse(renderMarkdownInline("`<x>`")).querySelector("code");
    expect(code?.textContent).toBe("<x>");
  });

  it("renders safe links with target/rel and drops unsafe ones", () => {
    const anchor = parse(
      renderMarkdownInline("[site](https://example.com)"),
    ).querySelector("a");
    expect(anchor?.getAttribute("href")).toBe("https://example.com");
    expect(anchor?.getAttribute("target")).toBe("_blank");
    expect(anchor?.getAttribute("rel")).toBe("noreferrer");
    expect(anchor?.textContent).toBe("site");

    const unsafe = parse(renderMarkdownInline("[danger](javascript:bad)"));
    expect(unsafe.querySelector("a")).toBeNull();
    expect(unsafe.body.textContent).toBe("danger");
  });

  it("linkifies embedded refs in inline text", () => {
    const span = parse(
      renderMarkdownInline("see rev:sha256:abcdef0123456789"),
    ).querySelector("span.ref");
    expect(span?.getAttribute("data-ref-kind")).toBe("rev");
  });

  it("honors backslash escapes outside inline code", () => {
    expect(
      renderMarkdownInline(
        "\\*literal\\*, \\`not code\\`, and `code \\* marker`",
      ),
    ).toBe("*literal*, `not code`, and <code>code \\* marker</code>");
  });
});

describe("renderContentHtml / renderBodyContent", () => {
  it("renders markdown vs plain text by content type", () => {
    expect(renderContentHtml("# H", "text/markdown")).toBe("<h1>H</h1>");
    const span = parse(
      renderContentHtml("rev:sha256:abcdef0123456789", "text/plain"),
    ).querySelector("span.ref");
    expect(span?.getAttribute("data-ref-kind")).toBe("rev");
  });

  it("wraps content in a div, with a markdown class only for markdown", () => {
    expect(renderBodyContent("", "text/plain")).toBe("");
    const md = parse(renderBodyContent("hi", "text/markdown")).querySelector(
      "div",
    );
    expect(md?.classList.contains("markdown-body")).toBe(true);
    expect(md?.classList.contains("anno-body")).toBe(true);
    const plain = parse(renderBodyContent("hi", "text/plain")).querySelector(
      "div",
    );
    expect(plain?.classList.contains("markdown-body")).toBe(false);
    expect(plain?.classList.contains("anno-body")).toBe(true);
  });
});
