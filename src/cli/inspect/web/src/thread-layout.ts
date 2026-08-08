// Neutral wire geometry for a server-computed supersession graph. This is kept
// apart from model.ts because both the retained aggregate reader and the active
// Change-first renderers consume it; geometry does not require legacy store
// state or revision derivation.

/** One laid-out node of a thread's supersession DAG (geometry + supersession state). */
export interface ThreadNode {
  id?: string;
  x?: number;
  y?: number;
  // Box dimensions and head/superseded state the DAG painter reads.
  w?: number;
  h?: number;
  isHead?: boolean;
  isSuperseded?: boolean;
}

/** The normalized (0,0)-origin bounds of a thread's laid-out graph. */
export interface ThreadBounds {
  w?: number;
  h?: number;
}

/** A routed supersession edge: the superseding `from`, the `to` it supersedes, and its polyline. */
export interface ThreadEdge {
  from?: string;
  to?: string;
  path?: number[][];
  /** The fact relation this edge encodes (`replaces`/`supersedes`); absent on revision edges. */
  kind?: string;
}

/** A thread's server-computed layout (the placed supersession nodes, edges, and bounds). */
export interface ThreadLayout {
  nodes?: ThreadNode[];
  edges?: ThreadEdge[];
  bounds?: ThreadBounds;
}
