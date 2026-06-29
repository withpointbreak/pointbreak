// The DOM access leaf: the `$` querySelector helper the impure modules share.
// Ported from the served app.js `$` (`const $ = (sel) => document.querySelector(sel)`),
// typed with `lib.dom` generics so callers get the concrete element type without a
// cast. Imports nothing.

/**
 * The first element matching `sel`, or `null` when nothing matches — the exact
 * `document.querySelector` contract the served app.js `$` carries. The generic
 * lets a caller name the element type it expects (`$<HTMLInputElement>("#filter-text")`)
 * and read its typed members without a cast or a non-null assertion.
 */
export function $<T extends Element = Element>(sel: string): T | null {
  return document.querySelector<T>(sel);
}
