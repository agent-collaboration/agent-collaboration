/** Read-only MCP proof for one ordinary CLI turn, without creating or closing a Session. */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { decode, encode } from '../../../../verification/exact-json.ts';
import {
	answered,
	callTool,
	observe,
	nextCursor,
	observeReplayBaseline,
	record,
} from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import type { JourneyContext } from '../../../../verification/observations/mcp-control/journeys/journey-steps.ts';
import { requireSelectedModel } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import { foldObservationReads } from '../../../../verification/observations/mcp-control/observations/observation-ledger.ts';
import type { ObservationRead } from '../../../../verification/observations/mcp-control/observations/observation-ledger.ts';
import type { SessionRef } from '../../../../verification/observations/mcp-control/observations/observation-models.ts';
import {
	agentReplyText,
	reconstructTurns,
} from '../../../../verification/observations/mcp-control/observations/observation-turns.ts';
import { StreamingMcpClient } from '../../../../verification/observations/mcp-control/transport/streaming-mcp-client.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { CODEX_0_165_TEXT } from './codex-acp-scenes.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';

const serviceSchema = z.looseObject({
	serviceId: z.string(),
	serviceEpoch: z.string(),
	mcp: z.looseObject({ url: z.string() }),
});

export type CliPromptDeclaration = Pick<
	Extract<CodexAcpRunPlan['journey'], { readonly kind: 'prompt' }>,
	'expected_model' | 'prompt' | 'completion' | 'reply_deadline_ms'
>;

export interface PromptReadbackHooks {
	readonly beforeInput: () => Promise<void>;
	readonly beforeSubmit: () => Promise<void>;
	readonly afterReply: (lines: readonly string[]) => Promise<void>;
}

export function cliMcpEndpoint(target: CodexAcpTarget): string {
	const service = serviceSchema.parse(
		decode(readFileSync(join(target.service_directory, 'service.json'), 'utf8')),
	);
	if (
		service.serviceId !== target.expected_service_id ||
		service.serviceEpoch !== target.expected_service_epoch
	)
		throw new Error('CLI MCP service identity or epoch changed');
	// Construction also refuses non-loopback and protected listeners before any connection.
	return new StreamingMcpClient(service.mcp.url).endpoint.href;
}

/** All reads share one exact target and one immutable deadline, and retain their raw exchanges. */
export function cliPromptReadback(inputs: {
	readonly target: CodexAcpTarget;
	readonly declaration: CliPromptDeclaration;
	readonly evidence: string;
}): PromptReadbackHooks {
	const { target, declaration, evidence } = inputs;
	const endpoint = cliMcpEndpoint(target);
	const context: JourneyContext = {
		client: new StreamingMcpClient(endpoint),
		evidence,
		deadline: Date.now() + declaration.reply_deadline_ms + 60_000,
	};
	const session: SessionRef = {
		endpoint: { serviceId: target.expected_service_id, endpointId: target.endpoint_id },
		sessionId: target.provider_session_id,
	};
	const reads: ObservationRead[] = [];
	let baselineReads = 0;
	const binding = (): void => {
		if (cliMcpEndpoint(target) !== endpoint) throw new Error('CLI MCP endpoint changed');
	};
	const model = async (step: string): Promise<void> => {
		binding();
		const outcome = await callTool(
			context,
			step,
			'provider_session_inspect',
			{ target: session },
			{ timeoutMs: 15_000 },
		);
		binding();
		requireSelectedModel(answered(outcome, step), session, declaration.expected_model);
	};
	async function retainReply(attempt: number): Promise<void> {
		if (attempt >= 10) throw new Error('CLI MCP turn remained unsettled at its read bound');
		binding();
		reads.push(
			await observe(
				context,
				`cli-reply-${attempt}`,
				session,
				nextCursor(session, reads, baselineReads),
				1,
			),
		);
		binding();
		const ledger = foldObservationReads(session, reads, baselineReads);
		if (ledger.kind !== 'continuous' || ledger.backendDisconnected || ledger.tooLarge.length > 0)
			throw new Error('CLI MCP reply history is incomplete');
		const reconstruction = reconstructTurns(ledger.events);
		if (reconstruction.kind === 'rejected') throw new Error(reconstruction.reason);
		if (reconstruction.acceptedInputs.length > 0 || reconstruction.turns.length > 1)
			throw new Error('another input or turn shared the CLI Session');
		const turn = reconstruction.turns[0];
		if (turn === undefined || turn.outcome === null) return retainReply(attempt + 1);
		if (
			turn.outcome.kind !== 'ended' ||
			turn.outcome.stop_reason !== 'endTurn' ||
			turn.outcome.local_cause !== null
		)
			throw new Error('CLI turn did not end normally');
		const users = turn.items.filter((item) => item.kind === 'userMessage');
		if (users.length !== 1 || users[0]?.text !== declaration.prompt)
			throw new Error('CLI MCP turn does not hold the exact prompt');
		const reply = agentReplyText(turn);
		if (reply.trim() !== declaration.completion)
			throw new Error('CLI MCP reply differs from the displayed completion');
		writeFileSync(
			join(evidence, 'cli-mcp-readback.json'),
			encode({
				target: session,
				inputId: turn.inputId,
				turnId: turn.turnId,
				outcome: turn.outcome,
				reply,
				displayedReply: declaration.completion,
				epoch: ledger.epoch,
				operationIdentifier: {
					kind: 'unobservable',
					reason: 'native CLI turns expose no retrievable operation identifier',
				},
			}),
			{ mode: 0o600, flag: 'wx' },
		);
		return;
	}
	return {
		beforeInput: async (): Promise<void> => {
			if (declaration.expected_model.trim() === '')
				throw new Error('a declared provider model is required');
			binding();
			const initialized = await context.client.initialize(15_000);
			record(context, { step: 'cli-mcp-initialize', outcome: initialized });
			answered(initialized, 'CLI MCP initialize');
			await model('cli-before-input-model');
			const baseline = await observeReplayBaseline(context, 'cli-baseline', session);
			binding();
			if (baseline === null) throw new Error('CLI MCP baseline exceeded its read bound');
			reads.push(...baseline);
			baselineReads = reads.length;
			const ledger = foldObservationReads(session, reads, baselineReads);
			if (
				ledger.kind !== 'continuous' ||
				ledger.backendDisconnected ||
				ledger.baselineTooLarge.length > 0
			)
				throw new Error('CLI MCP baseline is incomplete');
		},
		beforeSubmit: async (): Promise<void> => {
			if (baselineReads === 0) throw new Error('CLI MCP baseline is absent');
			await model('cli-before-enter-model');
		},
		afterReply: async (lines: readonly string[]): Promise<void> => {
			if (baselineReads === 0) throw new Error('CLI MCP baseline is absent');
			// The deciding reply is a whole line in both the original grid and MCP, never a marker in prose.
			if (
				!lines.some(
					(line) => line.trim() === `${CODEX_0_165_TEXT.infoPrefix}${declaration.completion}`,
				)
			)
				throw new Error('CLI frame lacks the exact completion line');
			await retainReply(0);
		},
	};
}
