import { createConnection } from "node:net";
import { connectionDenied } from "../tooling/socket-permission-probe.ts";

const path = process.argv[2];
if (path === undefined) throw new Error("Unix socket path required");
const socket = createConnection(path);
socket.once("connect", (): void => {
  socket.destroy();
});
socket.once("error", (error): void => {
  console.error(connectionDenied(error) ? "PermissionError" : error);
  process.exitCode = 1;
});
