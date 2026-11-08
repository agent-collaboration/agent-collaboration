/** Compare central's original Push arrival order per declared sender; agent order is D5. */
import { z } from "zod";

const submittedSchema = z
  .array(z.object({ item: z.uuidv7(), sender: z.string().min(1) }).strict())
  .min(1);
export type SubmittedPushOrder = z.infer<typeof submittedSchema>;

/** Supply item IDs from the qualified original SessionAudit, including retries. */
export function observePerSenderPushOrder(input: {
  readonly submitted: SubmittedPushOrder;
  readonly observedItems: readonly string[];
}): { firstSends: readonly string[]; retries: number; runtimeQualified: false } {
  const submitted = submittedSchema.parse(input.submitted);
  const observed = z.array(z.uuidv7()).parse(input.observedItems);
  const owners = new Map(submitted.map((item): [string, string] => [item.item, item.sender]));
  if (owners.size !== submitted.length) throw new Error("duplicate submitted Push item");
  const firstSends: string[] = [],
    seen = new Set<string>();
  for (const item of observed) {
    if (!owners.has(item)) throw new Error("unrequested Push item in the scoped wire capture");
    if (!seen.has(item)) {
      seen.add(item);
      firstSends.push(item);
    }
  }
  if (seen.size !== submitted.length)
    throw new Error("wire capture is missing submitted Push items");
  for (const sender of new Set(owners.values())) {
    const expected = submitted
      .filter((item): boolean => item.sender === sender)
      .map((item): string => item.item);
    const actual = firstSends.filter((item): boolean => owners.get(item) === sender);
    if (actual.some((item, index): boolean => item !== expected[index]))
      throw new Error(`central Push order differs for sender ${sender}`);
  }
  return { firstSends, retries: observed.length - firstSends.length, runtimeQualified: false };
}
