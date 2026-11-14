/** The values `cairn-lang-wasm` answers, and the messages the page and its worker exchange. */

export type Severity = "error" | "warning";

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
  | { id: number; kind: "compile"; source: string; edition: string; target: string }
  | { id: number; kind: "targets"; edition: string };

export type Response =
  | { kind: "ready"; version: string }
  | { kind: "unavailable" }
  | { kind: "answer"; id: number; value: Compiled | Targets }
  | { kind: "failed"; id: number; message: string; trapped: boolean };
