/** The `cairn.edition` and `cairn.target` a configuration change carried. */
export interface Pin {
  readonly edition?: string;
  readonly target?: string;
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
