import { createPrivateKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import { z } from "zod";
import { fileSchema } from "../declaration.ts";
import { fingerprint } from "../machines/owned-process.ts";

export async function fixtureMachineAssertion(input: {
  key: z.infer<typeof fileSchema>;
  accessClientId: string;
  expiresAt: string;
}): Promise<string> {
  if ((await fingerprint(input.key.path)) !== input.key.sha256)
    throw new Error("fixture key differs from declaration");
  const key = createPrivateKey({
    key: await readFile(input.key.path),
    format: "der",
    type: "pkcs8",
  });
  const header = Buffer.from(
    JSON.stringify({ alg: "RS256", typ: "JWT", kid: "access-test-key" }),
  ).toString("base64url");
  const claims = Buffer.from(
    JSON.stringify({
      iss: "https://conformance-test.cloudflareaccess.com",
      aud: "test-session-aud",
      exp: Math.floor(Date.parse(input.expiresAt) / 1000),
      iat: Math.floor(Date.now() / 1000),
      type: "app",
      sub: "",
      common_name: input.accessClientId,
    }),
  ).toString("base64url");
  const signed = `${header}.${claims}`;
  return `${signed}.${sign("RSA-SHA256", Buffer.from(signed), key).toString("base64url")}`;
}
