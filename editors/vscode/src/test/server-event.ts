/**
 * A line a stand-in appends to the shared record: its launch, a lifecycle
 * method it received, or its exit code.
 */
export type ServerEvent =
  | { readonly id: string; readonly server: string; readonly argv: string[] }
  | { readonly id: string; readonly received: string }
  | { readonly id: string; readonly exited: number };
