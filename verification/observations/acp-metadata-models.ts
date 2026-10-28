import { z } from "zod";
import { exactIntegerSchema, sessionViewSchema } from "./evidence-models.ts";

export const metadataObservationPlanSchema = z.strictObject({
  repository: z.string(),
  source_head: z.string(),
  build_manifest: z.string(),
  host_pid: z.coerce.number().int().positive(),
  host_binary: z.string(),
  host_sha256: z.string(),
  router_binary: z.string(),
  router_sha256: z.string(),
  codex_cli: z.string(),
  codex_sha256: z.string(),
  collaboration_cli: z.string(),
  service_directory: z.string(),
  endpoint_id: z.enum(["claude-local", "cursor-local"]),
  session_id: z.string(),
  terminal_capture: z.string(),
  resume_capture: z.string(),
  expected_reply: z.string().min(1),
  evidence: z.string(),
  require_usage: z.boolean().default(false),
});
export type MetadataObservationPlan = z.infer<typeof metadataObservationPlanSchema>;
export const normalBuildManifestSchema = z.object({
  source_commit: z.string(),
  source_tree: z.string(),
  dirty: z.boolean(),
  test_guard_features: z.array(z.string()),
  binaries: z.record(z.string(), z.object({ path: z.string(), sha256: z.string() })),
});
export const providerItemSchema = z.object({
  itemId: z.string(),
  kind: z.object({ kind: z.string() }),
  text: z.string().nullable().default(null),
});
export type ProviderItem = z.infer<typeof providerItemSchema>;
export const providerObservationSchema = z.object({
  target: sessionViewSchema,
  events: z.array(
    z.object({
      sequence: exactIntegerSchema,
      event: z.object({
        kind: z.string(),
        item: providerItemSchema.nullable().default(null),
        turnId: z.string().nullable().default(null),
      }),
    }),
  ),
  epoch: exactIntegerSchema,
  endReason: z.string(),
});
export type ProviderObservation = z.infer<typeof providerObservationSchema>;
export const codexMetadataThreadSchema = z.object({
  id: z.string(),
  turns: z.array(
    z.object({
      id: z.string(),
      items: z.array(
        z.object({ id: z.string(), type: z.string(), text: z.string().nullable().default(null) }),
      ),
    }),
  ),
});
export type CodexMetadataThread = z.infer<typeof codexMetadataThreadSchema>;
export const metadataObservationResultSchema = z.object({
  thread_id: z.string(),
  metadata_item_ids: z.array(z.string()),
  metadata_kinds: z.array(z.string()),
  command_catalogues: z.number().int(),
  usage_items: z.number().int(),
  assistant_item_ids: z.array(z.string()),
  historical_views: z.number().int(),
  terminal_reply_observed: z.boolean(),
  terminal_views: z.number().int(),
  usage_limit: z.string().nullable(),
});
export type MetadataObservationResult = z.infer<typeof metadataObservationResultSchema>;
