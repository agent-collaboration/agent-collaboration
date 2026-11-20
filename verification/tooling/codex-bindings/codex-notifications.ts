import { z } from "zod";

import type { RustBindingItem } from "./codex-rust-binding.ts";
import { definitionName, definitionRef } from "./codex-schema.ts";

export const selectedNotifications = [
  "turn/started",
  "turn/completed",
  "item/started",
  "item/completed",
  "item/agentMessage/delta",
];

const notificationUnion = z.object({ oneOf: z.array(z.unknown()) });
const notificationMethod = z.object({
  properties: z.object({ method: z.object({ enum: z.array(z.unknown()) }) }),
});
const notificationEnvelope = z.object({
  type: z.literal("object"),
  required: z.array(z.enum(["method", "params"])).length(2),
  properties: z.object({
    method: z.object({ type: z.literal("string"), enum: z.tuple([z.string()]) }),
    params: z.strictObject({ $ref: definitionRef }),
  }),
});

interface NotificationBinding {
  readonly method: string;
  readonly params: string;
}

function resolveNotification(envelopes: readonly unknown[], method: string): NotificationBinding {
  const matches = envelopes.filter((envelope) => {
    const discriminator = notificationMethod.safeParse(envelope);
    return discriminator.success && discriminator.data.properties.method.enum.includes(method);
  });
  if (matches.length !== 1) {
    throw new Error(
      `${method}: expected exactly one ServerNotification envelope, found ${matches.length}`,
    );
  }
  const envelope = notificationEnvelope.safeParse(matches[0]);
  if (!envelope.success || new Set(envelope.data.required).size !== 2) {
    throw new Error(`${method}: invalid ServerNotification envelope`);
  }
  const { params } = envelope.data.properties;
  return { method: envelope.data.properties.method.enum[0], params: params.$ref };
}

export function resolveNotifications(source: unknown): readonly NotificationBinding[] {
  const union = notificationUnion.safeParse(source);
  if (!union.success) {
    const methods = selectedNotifications.join(", ");
    throw new Error(`${methods}: missing or malformed ServerNotification.oneOf`);
  }
  const methodByNotificationParams = new Map<string, string>();
  return selectedNotifications.map((method) => {
    const notification = resolveNotification(union.data.oneOf, method);
    const previousMethod = methodByNotificationParams.get(notification.params);
    if (previousMethod !== undefined) {
      throw new Error(
        `ServerNotification methods ${previousMethod} and ${method} share params ${notification.params}`,
      );
    }
    methodByNotificationParams.set(notification.params, method);
    return notification;
  });
}

export function groupNotificationItems(
  items: readonly RustBindingItem[],
  bindings: readonly NotificationBinding[],
): { declarations: readonly RustBindingItem[]; notifications: readonly RustBindingItem[] } {
  const methodByPayload = new Map(
    bindings.map(({ method, params }) => [definitionName(params), method]),
  );
  const declarations: RustBindingItem[] = [];
  const notifications: RustBindingItem[] = [];
  for (const item of items) {
    const method = methodByPayload.get(item.name);
    if (method === undefined) {
      declarations.push(item);
      continue;
    }
    notifications.push({
      ...item,
      source: [
        item.source,
        "",
        `impl super::sealed::Sealed for ${item.name} {}`,
        "",
        `impl super::CodexServerNotification for ${item.name} {`,
        `const METHOD: &'static str = "${method}";`,
        "}",
      ].join("\n"),
    });
  }
  return { declarations, notifications };
}
