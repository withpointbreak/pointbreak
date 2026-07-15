const SHORT_ID_LENGTH = 12;

/** Compact a typed content-addressed reference for human-facing labels. */
export function shortReferenceId(id: string): string {
  const value = id.split(":").at(-1) ?? id;
  return value.slice(0, SHORT_ID_LENGTH);
}
