/** node scripts/verification/tui/terminal/ghostty-input-qualification.ts OUTPUT.json
 * Qualifies apparatus against controlled VT modes. Never launches the product.
 */
import { writeFileSync } from 'node:fs';

import { layerCKeyboardKeys, layerCProfiles } from '../capture/harness-journeys.ts';
import { isMain, planArgument, reportCliFailure, strictArguments } from '../cli/cli-arguments.ts';
import { journeyGridSizes } from '../journeys/journey-definition.ts';
import { readGhosttyInputModes } from './ghostty-input-encoding.ts';
import { GhosttyJourneyInput } from './ghostty-journey-input.ts';
import type { InputDeliveryRecord } from './ghostty-journey-input.ts';
import { createTerminalCore } from './terminal-grid.ts';
import { qualifyTerminal } from './terminal-qualification.ts';
// One mutable VT core: steps must be serial, not concurrent calls over changing modes.
async function inSequence<TItem>(
	items: readonly TItem[],
	observe: (item: TItem) => Promise<void>,
): Promise<void> {
	await items.reduce<Promise<void>>(async (previous, item) => {
		await previous;
		await observe(item);
	}, Promise.resolve());
}

export async function qualifyGhosttyInput(): Promise<{
	readonly kind: 'ghostty-input-apparatus-qualification';
	readonly productPasses: 0;
	readonly modesSource: 'controlled VT sequences, not product output';
	readonly encoderRoute: 'kitty specification plus pinned Ghostty v1.3.1 source/test literals';
	readonly emulator: Awaited<ReturnType<typeof qualifyTerminal>>;
	readonly records: readonly InputDeliveryRecord[];
	readonly unverified: readonly string[];
}> {
	const emulator = await qualifyTerminal();
	const core = await createTerminalCore(120, 36);
	const records: InputDeliveryRecord[] = [];
	try {
		await inSequence([0, 1, 31], async (flags) => {
			core.writeString(`\x1b[>${flags}u\x1b[?1002h\x1b[?1006h`);
			await inSequence(layerCProfiles, async (profile) => {
				const input = new GhosttyJourneyInput(
					{ inputModes: () => readGhosttyInputModes(core), send: async (): Promise<void> => {} },
					profile,
				);
				await inSequence(layerCKeyboardKeys, async (key) => {
					await input.key(key);
				});
				await inSequence(journeyGridSizes, async (dimensions) => {
					const [columns, rows] = dimensions.split('x').map(Number);
					if (columns === undefined || rows === undefined)
						throw new Error('missing grid dimension');
					core.resize(columns, rows);
					await input.click(columns - 1, rows - 1);
					await input.mouse({ kind: 'wheel', direction: 'up', column: 0, row: 0 });
					await input.mouse({ kind: 'wheel', direction: 'down', column: 0, row: 0 });
				});
				records.push(...input.history());
			});
			core.writeString('\x1b[<u');
		});
		return {
			kind: 'ghostty-input-apparatus-qualification',
			productPasses: 0,
			modesSource: 'controlled VT sequences, not product output',
			encoderRoute: 'kitty specification plus pinned Ghostty v1.3.1 source/test literals',
			emulator,
			records,
			unverified: [
				'S9 real picker/quota input and causal overlay-isolation observations',
				'physical keyboard hardware',
				'Ghostty app keybinds beyond the modelled set',
				'OS-level shortcuts',
				'real Ghostty rendering beyond libghostty',
			],
		};
	} finally {
		core.dispose();
	}
}
if (isMain(import.meta.url)) {
	try {
		const { positionals } = strictArguments(process.argv.slice(2), {}, true);
		const output = planArgument(positionals);
		const receipt = await qualifyGhosttyInput();
		writeFileSync(output, JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
		process.stdout.write(
			`qualified ${receipt.records.length} input deliveries; productPasses=0; ${output}\n`,
		);
	} catch (error) {
		reportCliFailure(error);
	}
}
