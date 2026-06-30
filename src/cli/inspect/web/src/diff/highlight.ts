// Pure, fail-safe syntax-token emit for a diff row. Slices the raw row text by
// UTF-16 offsets (matching the wire span offsets), escapes each segment, and wraps
// non-plain segments in their token-kind class. A leaf module: no DOM, no state.

import { tokClass } from "../classNames";
import { escapeHtml } from "../escape";

/** A token span over a row's text. `start`/`end` are UTF-16 code-unit offsets. */
export interface TokenSpan {
  start: number;
  end: number;
  kind: string;
}

/** A channel is valid when its spans are integer, sorted, non-overlapping, in range. */
function validChannel(spans: TokenSpan[], len: number): boolean {
  let cursor = 0;
  for (const span of spans) {
    if (
      !Number.isInteger(span.start) ||
      !Number.isInteger(span.end) ||
      span.start < cursor ||
      span.end < span.start ||
      span.end > len
    ) {
      return false;
    }
    cursor = span.end;
  }
  return true;
}

/**
 * Render a row's text with syntax tokens. With no tokens (or a malformed span set)
 * this returns exactly `escapeHtml(text)`, so an unhighlighted row is byte-identical
 * to the plain renderer.
 *
 * The emit is a left-to-right attributed-segment sweep: each token span becomes one
 * escaped, wrapped segment and the gaps between them stay plain. A future second
 * channel (intraline diff) contributes its own boundaries and an extra class per
 * segment to this same sweep — callers do not change.
 */
export function highlightRowText(text: string, tokens?: TokenSpan[]): string {
  if (!tokens || tokens.length === 0) return escapeHtml(text);
  if (!validChannel(tokens, text.length)) return escapeHtml(text);

  let cursor = 0;
  let out = "";
  for (const token of tokens) {
    if (token.start > cursor)
      out += escapeHtml(text.slice(cursor, token.start));
    out += `<span class="${tokClass(token.kind)}">${escapeHtml(
      text.slice(token.start, token.end),
    )}</span>`;
    cursor = token.end;
  }
  if (cursor < text.length) out += escapeHtml(text.slice(cursor));
  return out;
}
