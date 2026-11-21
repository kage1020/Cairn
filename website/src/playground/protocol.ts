/** The values `cairn-lang-wasm` answers, and the messages the page and its worker exchange. */

export type Severity = "error" | "warning";

export type Edition = "java" | "bedrock";

export interface Note {
  line?: number;
  col?: number;
  message: string;
}

/** A finding, in the shape `spec/lint` "Machine-readable payload" gives. */
export interface Diagnostic {
  code: string;
  severity: Severity;
  line: number;
  col: number;
  end_line: number;
  end_col: number;
  primary: string;
  notes?: Note[];
  /** The fields a consumer would otherwise parse out of the message, under the code's `kind`. */
  data?: { kind: string } & Record<string, unknown>;
}

/** A run-level refusal, or a warning that names no span. */
export interface RunLevelMessage {
  code?: string;
  severity: Severity;
  line?: number;
  col?: number;
  primary: string;
  notes?: Note[];
}

export interface BlockState {
  id: string;
  properties?: Record<string, string>;
}

export interface BlockArray {
  dims: { x: number; y: number; z: number };
  palette: BlockState[];
  /** Palette indices in `(y, z, x)` order: `(y * dims.z + z) * dims.x + x` for `(x, y, z)`. */
  voxels: number[];
  source_scope: string;
}

export interface BlockArrayIr {
  structures: Record<string, BlockArray>;
  /** Each structure's members, which the page shows only as JSON. */
  members: Record<string, unknown[]>;
}

export interface Artifact {
  name: string;
  bytes: Uint8Array;
}

export interface Compiled {
  ok: boolean;
  diagnostics: Diagnostic[];
  run_level: RunLevelMessage[];
  ir: BlockArrayIr | null;
  artifacts: Artifact[];
}

export interface Targets {
  latest: string;
  versions: string[];
}

export type Request =
  | { id: number; kind: "compile"; source: string; edition: Edition; target: string }
  | { id: number; kind: "targets"; edition: Edition };

/** What the worker answers a request of each kind with. */
export interface Answers {
  compile: Compiled;
  targets: Targets;
}

export type Response =
  | { kind: "ready"; version: string }
  | { kind: "unavailable" }
  | { kind: "load-failed"; message: string }
  | { kind: "answer"; id: number; value: Answers[keyof Answers] }
  | { kind: "failed"; id: number; message: string; trapped: boolean };
