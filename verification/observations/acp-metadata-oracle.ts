import { evidenceJson } from "../evidence-json.ts";
import { readObservedJson } from "./evidence-models.ts";
import type {
  CodexMetadataThread,
  MetadataObservationResult,
  ProviderItem,
  ProviderObservation,
} from "./acp-metadata-models.ts";

export function terminalText(raw: Buffer): string {
  const escape = String.fromCharCode(27);
  const bell = String.fromCharCode(7);
  const text = raw
    .toString("utf8")
    .replaceAll("\r\n", "\n")
    .replace(new RegExp(`${escape}\\[[0-?]*[ -/]*[@-~]`, "g"), "");
  return text.replace(
    new RegExp(`${escape}\\][^${bell}${escape}]*(?:${bell}|${escape}\\\\)`, "g"),
    "",
  );
}

function escapedRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function isCommandCatalogue(item: ProviderItem): boolean {
  if (item.kind.kind !== "notice" || item.text === null) return false;
  try {
    return Array.isArray(readObservedJson(item.text).availableCommands);
  } catch {
    return false;
  }
}

export function requireTerminalProse(
  capture: string,
  expectedReply: string,
  metadata: readonly ProviderItem[],
): void {
  if (
    !new RegExp(`^\\s*•[ \\t]+${escapedRegex(expectedReply)}(?=[ \\t]*(?:$|›))`, "m").test(capture)
  )
    throw new Error("ordinary assistant reply absent from original installed-client PTY");
  const compactCapture = capture.replace(/\s+/g, "");
  for (const item of metadata) {
    if (item.text === null) continue;
    let keys: string[];
    try {
      keys = Object.keys(readObservedJson(item.text));
    } catch {
      if (new RegExp(`^\\s*•\\s+${escapedRegex(item.text)}\\s*$`, "m").test(capture))
        throw new Error("provider metadata appeared as terminal prose");
      continue;
    }
    if (keys.some((key) => compactCapture.includes(`${evidenceJson(key)}:`.replace(/\s+/g, ""))))
      throw new Error("raw provider metadata appeared in installed-client PTY");
  }
}

export interface MetadataOracleInput {
  readonly observation: ProviderObservation;
  readonly views: readonly CodexMetadataThread[];
  readonly terminal: string;
  readonly resumedTerminal: string;
  readonly expectedReply: string;
  readonly requireUsage: boolean;
}

export function checkMetadataObservation(input: MetadataOracleInput): MetadataObservationResult {
  const { observation, views, terminal, resumedTerminal, expectedReply, requireUsage } = input;
  if (observation.endReason !== "deadlineReached")
    throw new Error("observation incomplete: page, reset or disconnect needs reconciliation");
  const retained = new Map<string, ProviderItem>();
  for (const entry of observation.events) {
    const event = entry.event;
    if ((event.kind === "itemStarted" || event.kind === "itemUpdated") && event.item !== null)
      retained.set(event.item.itemId, event.item);
  }
  const metadata = [...retained.values()].filter(
    (item) =>
      ["usage", "modeChange", "configChange", "sessionInfo"].includes(item.kind.kind) ||
      isCommandCatalogue(item),
  );
  const commands = metadata.filter(isCommandCatalogue).length;
  const usage = metadata.filter((item) => item.kind.kind === "usage").length;
  if (commands === 0 || (requireUsage && usage === 0))
    throw new Error("required provider metadata was not retained");
  const metadataIds = new Set(metadata.map((item) => item.itemId));
  const assistants = [...retained.values()].filter(
    (item) => item.kind.kind === "agentMessage" && item.text === expectedReply,
  );
  if (assistants.length === 0)
    throw new Error("ordinary assistant reply absent from original provider events");
  const firstView = views[0];
  if (
    views.length !== 2 ||
    firstView === undefined ||
    views.some((view) => view.id !== observation.target.sessionId)
  )
    throw new Error("both read and resume views of the same thread are required");
  const assistantIds = new Set(assistants.map((item) => item.itemId));
  for (const view of views) {
    const rendered = view.turns.flatMap((turn) => turn.items);
    if (rendered.some((item) => metadataIds.has(item.id)))
      throw new Error("provider metadata leaked into a Codex transcript item");
    if (
      !rendered.some(
        (item) =>
          item.type === "agentMessage" && assistantIds.has(item.id) && item.text === expectedReply,
      )
    )
      throw new Error("ordinary assistant reply missing from Codex history");
  }
  for (const capture of [terminal, resumedTerminal])
    requireTerminalProse(capture, expectedReply, metadata);
  return {
    thread_id: firstView.id,
    metadata_item_ids: [...metadataIds].sort(),
    metadata_kinds: [...new Set(metadata.map((item) => item.kind.kind))].sort(),
    command_catalogues: commands,
    usage_items: usage,
    assistant_item_ids: [...assistantIds].sort(),
    historical_views: views.length,
    terminal_reply_observed: true,
    terminal_views: 2,
    usage_limit:
      usage > 0 ? null : "Provider emitted no usage; context/cost runtime proof remains unobserved",
  };
}
