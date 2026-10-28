import assert from "node:assert/strict";
import { assertConnectionDenied } from "../tooling/socket-permission-probe.ts";

const [, , socket, port, marker] = process.argv;
assert.ok(socket !== undefined);
assert.ok(port !== undefined && /^[0-9]+$/.test(port));
assert.equal(marker, "NARROW_ACCESS_DENIED");
await assertConnectionDenied({ kind: "unix", path: socket }, marker);
await assertConnectionDenied({ kind: "tcp", port: Number(port) }, marker);
console.log(marker);
