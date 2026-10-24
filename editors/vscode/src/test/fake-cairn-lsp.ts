import { randomUUID } from "node:crypto";
import { appendFileSync } from "node:fs";
import type { ServerEvent } from "./server-event";

type Message = { readonly id?: number | string; readonly method?: string };

const [log, server, ...argv] = process.argv.slice(2);
const id = randomUUID();

function record(event: ServerEvent): void {
  appendFileSync(log, `${JSON.stringify(event)}\n`);
}

function send(message: object): void {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

function handle(message: Message): void {
  switch (message.method) {
    case "initialize":
      record({ id, received: message.method });
      send({ jsonrpc: "2.0", id: message.id, result: { capabilities: {} } });
      break;
    case "initialized":
      record({ id, received: message.method });
      break;
    case "shutdown":
      record({ id, received: message.method });
      send({ jsonrpc: "2.0", id: message.id, result: null });
      break;
    case "exit":
      record({ id, received: message.method });
      process.exit(0);
    default:
      if (message.id !== undefined && message.method !== undefined) {
        const error = { code: -32601, message: "Method not found" };
        send({ jsonrpc: "2.0", id: message.id, error });
      }
  }
}

record({ id, server, argv });
process.on("exit", (code) => record({ id, exited: code }));

if (argv.length === 1 && argv[0] === "--version") {
  process.stdout.write("cairn-lsp 0.0.0\n");
  process.exit(0);
}

let pending = Buffer.alloc(0);
process.stdin.on("data", (chunk: Buffer) => {
  pending = Buffer.concat([pending, chunk]);
  for (;;) {
    const headerEnd = pending.indexOf("\r\n\r\n");
    if (headerEnd < 0) {
      return;
    }
    const header = pending.toString("ascii", 0, headerEnd);
    const contentLength = /content-length: *(\d+)/i.exec(header);
    if (!contentLength) {
      throw new Error(`no Content-Length in header ${JSON.stringify(header)}`);
    }
    const length = Number(contentLength[1]);
    const bodyStart = headerEnd + 4;
    if (pending.length < bodyStart + length) {
      return;
    }
    const body = pending.toString("utf8", bodyStart, bodyStart + length);
    pending = pending.subarray(bodyStart + length);
    handle(JSON.parse(body) as Message);
  }
});
