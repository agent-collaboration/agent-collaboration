import { createConnection } from "node:net";

export function connectionDenied(error: unknown): boolean {
  return (
    error !== null &&
    typeof error === "object" &&
    "code" in error &&
    (error.code === "EPERM" || error.code === "EACCES")
  );
}

export async function requireConnectionDenied(
  endpoint:
    | { readonly kind: "unix"; readonly path: string }
    | { readonly kind: "tcp"; readonly port: number },
  marker: string,
): Promise<void> {
  await assertConnectionDenied(endpoint, marker);
  console.log(marker);
}

export async function assertConnectionDenied(
  endpoint:
    | { readonly kind: "unix"; readonly path: string }
    | { readonly kind: "tcp"; readonly port: number },
  marker: string,
): Promise<void> {
  await new Promise<void>((resolve, reject): void => {
    const connection =
      endpoint.kind === "unix"
        ? createConnection(endpoint.path)
        : createConnection({ host: "127.0.0.1", port: endpoint.port });
    connection.once("connect", (): void => {
      connection.destroy();
      reject(new Error(`Sandbox unexpectedly allowed ${marker}`));
    });
    connection.once("error", (error: Error): void => {
      if (!connectionDenied(error)) {
        reject(error);
        return;
      }
      resolve();
    });
    connection.setTimeout(2000, (): void => {
      connection.destroy();
      reject(new Error("connection timed out"));
    });
  });
}

export async function main(): Promise<void> {
  const [, , socketPath, portText] = process.argv;
  if (process.argv.length !== 4 || socketPath === undefined || portText === undefined)
    throw new Error("Usage: socket-permission-probe.ts UNIX_SOCKET TCP_PORT");
  const port = Number(portText);
  if (!/^[+-]?[0-9]+$/.test(portText.trim()) || !Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("TCP port must be 1..65535");
  await requireConnectionDenied({ kind: "unix", path: socketPath }, "SOCKET_BOUNDARY_OK");
  await requireConnectionDenied({ kind: "tcp", port }, "TCP_BOUNDARY_OK");
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
