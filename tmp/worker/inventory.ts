import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const crates = `acp-client-runtime agent-automation agent-collaboration automation-storage board-central-test-support board-fleet-sync board-legacy-upgrade board-sync-protocol claude-code-peer-messaging codex-acp-adapter codex-router-core codex-router-host codex-router-secret-store codex-router-selection codex-router-state collaboration-central-admin-protocol collaboration-central-cloudflare collaboration-central-conformance collaboration-central-core collaboration-contracts collaboration-mail-protocol collaboration-mcp collaboration-protocol collaboration-runtime collaboration-service fleet-admission interaction-storage lifecycle-observation message-board message-board-storage session-event-model tailnet-verification-host workspace-architecture-check`.split(' ');
const rows: string[] = [];
const contextRows: string[] = [];
let occurrences = 0;
function visit(directory: string) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) { visit(path); continue; }
    if (!/\.(rs|ts|js|sh)$/.test(path)) continue;
    const lines = readFileSync(path, 'utf8').split('\n');
    const groups = new Map<string, number[]>();
    lines.forEach((line, index) => {
      let classification: string | undefined;
      if (/(?<![\w])(sleep|sleep_until|interval|interval_at|setTimeout|setInterval)\s*\(/.test(line) || /\bsleep\s+\d/.test(line)) {
        classification = /sleep_until\(deadline\)/.test(line) ? 'C or B: inspect deadline owner; retain causal guard, inject behavioural deadline' : 'A or B: inspect owner; replace polling with signal or drive injected time';
      } else if (/\b(timeout|timeout_at|recv_timeout|wait_timeout_while)\s*\(/.test(line)) {
        classification = 'C: retain bound around causal wait; inspect negative timeout assertions for B';
      } else if (/\belapsed\s*\(/.test(line)) {
        classification = 'B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state';
      } else if (/\b(?:Instant|SystemTime|Utc|Local)::now\s*\(/.test(line)) {
        classification = 'context: timestamp or deadline origin; inspect consumer for elapsed-time dependence';
      }
      if (!classification) return;
      occurrences += 1;
      const group = groups.get(classification) ?? [];
      group.push(index + 1);
      groups.set(classification, group);
    });
    for (const [classification, locations] of groups) {
      const row = `| ${path}:${locations.join(',')} | ${classification} |`;
      (classification.startsWith('context:') ? contextRows : rows).push(row);
    }
  }
}
for (const crate of crates) visit(join('crates', crate));
writeFileSync('tmp/worker/inventory.md', `# W1 timing inventory\n\nSource: 881f778c77b4d5532ef1be32f0bd2f44daaf73f5; generated before product edits.\n\nScope is the exact W1 list in Lead push 01a18721-cad9-7d81-8587-6e18cc86082f. W2, proxy, and verification TypeScript are excluded. The scan includes production timing owners so test dependencies are visible. Production scheduling remains unchanged unless an approved clock injection makes tests deterministic.\n\nA = sleep/poll synchronization, B = real-duration behaviour, C = bound around causal wait. Initial ambiguous scan entries require source classification before conversion; timestamps used only as fixture data do not require elapsed time and are listed separately.\n\n${occurrences} occurrences across ${rows.length + contextRows.length} grouped rows. Every comma-separated number is an original source line.\n\n## Waits, scheduling, elapsed-time uses\n\n| Source lines | Classification and plan |\n| --- | --- |\n${rows.join('\n')}\n\n## Timestamp and deadline origins\n\nThese are context candidates, not automatically violations: inspect the consumer; replace behavioural wall time with injected/fixed time, retain timestamp data that never needs time to pass.\n\n| Source lines | Plan |\n| --- | --- |\n${contextRows.join('\n')}\n`);
writeFileSync('tmp/worker/scope.txt', crates.join('\n') + '\n');
process.stdout.write(JSON.stringify({ occurrences, waitRows: rows.length, timestampRows: contextRows.length }) + '\n');
