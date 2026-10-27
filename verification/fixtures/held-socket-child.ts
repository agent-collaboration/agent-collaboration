import { once } from "node:events";
import { createConnection } from "node:net";
import { requireArgument } from "./acp-stdio.ts";
const socket = createConnection(requireArgument(2));
await once(socket, "connect");
socket.write("R");
setInterval((): void => {}, 60_000);
