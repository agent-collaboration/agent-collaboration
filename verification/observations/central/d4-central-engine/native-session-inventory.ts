/** The native shell's existing owner-authenticated route, outside the general admin client. */
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";

export async function readNativeSessionInventory(input: {
  readonly origin: string;
  readonly assertion: string;
  readonly service: string;
}): Promise<unknown> {
  const origin = new URL(input.origin);
  if (
    origin.protocol !== "http:" ||
    origin.hostname !== "127.0.0.1" ||
    Number(origin.port) < 1024 ||
    [8787, 8788, 19741, 19742, 19743, 43127].includes(Number(origin.port)) ||
    origin.pathname !== "/" ||
    origin.username !== "" ||
    origin.password !== "" ||
    origin.search !== "" ||
    origin.hash !== ""
  )
    throw new Error("native inventory requires an isolated declared loopback central");
  const reply = await fetch(new URL("/__conformance/admission", origin), {
    method: "POST",
    redirect: "error",
    signal: AbortSignal.timeout(30_000),
    headers: {
      "Content-Type": "application/json",
      "User-Agent": "codex-router-verification/1.0",
      "Cf-Access-Jwt-Assertion": z.string().min(1).parse(input.assertion),
    },
    body: encode({ kind: "sessionConnections", value: { service: z.uuid().parse(input.service) } }),
  });
  if (reply.status !== 200) throw new Error(`native owner inventory HTTP status ${reply.status}`);
  const contents = await reply.text();
  if (Buffer.byteLength(contents) > 16 * 1024 * 1024)
    throw new Error("native inventory exceeds bound");
  return decode(contents);
}
