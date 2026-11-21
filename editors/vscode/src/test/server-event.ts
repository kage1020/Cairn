/** The `cairn.edition` and `cairn.target` a configuration change carried. */
export interface Pin {
  readonly edition?: string;
  readonly target?: string;
}

/**
 * How a test runs `Cairn: Show Info`: the file it writes what the command did
 * to, as `{ opened?, message? }` JSON, and the editor it runs in, which is the
 * `.crn` file the window opened unless it names a new `plaintext` document or
 * an `untitled` `.crn` one.
 */
export interface ShowInfo {
  readonly to: string;
  readonly editor?: "plaintext" | "untitled";
}

/**
 * How a test runs `Cairn: Diff Blocks`: the file it writes what the command
 * did to, as `{ opened?, message? }` JSON, and the structure file it compares
 * the `.crn` file the window opened with.
 */
export interface DiffBlocks {
  readonly to: string;
  readonly structure: string;
}

/**
 * A line a stand-in appends to the shared record: its launch, a lifecycle
 * method it received, the client commands `initialize` declared, the pin a
 * configuration change carried, or its exit code.
 */
export type ServerEvent =
  | { readonly id: string; readonly server: string; readonly argv: string[] }
  | { readonly id: string; readonly received: string }
  | { readonly id: string; readonly commands: string[] }
  | { readonly id: string; readonly pin: Pin }
  | { readonly id: string; readonly exited: number };
