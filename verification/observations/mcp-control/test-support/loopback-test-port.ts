// Test servers must not publish an OS-assigned port that the real client refuses to address.
import { PROTECTED_PORTS } from "../transport/streaming-mcp-client.ts";

interface TestServerBinding {
  listen(port: number, host: string, listening: () => void): unknown;
  address(): { readonly port: number } | string | null;
  close(closed: (error?: Error) => void): unknown;
}

export async function listenOnTestPort(server: TestServerBinding): Promise<number> {
  for (let bindingAttempt = 0; bindingAttempt < 8; bindingAttempt += 1) {
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("no test TCP port");
    if (!PROTECTED_PORTS.has(address.port)) return address.port;
    await new Promise<void>((resolve, reject) => {
      server.close((error) => (error === undefined ? resolve() : reject(error)));
    });
  }
  throw new Error("no unprotected test port after 8 bindings");
}
