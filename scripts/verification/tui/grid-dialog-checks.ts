import type { GridRectangle } from './grid-scene-matcher.ts';
/** Semantic fields and choices belong to the active dialog, including their style. */
import { cellText, terminalTheme } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
const normalize = (text: string): string => text.replace(/\s+/gu, ' ').trim().toLowerCase();
export function dialogSemanticFailures(
	grid: TerminalGrid,
	checkpoint: string,
	rect: GridRectangle | null,
): readonly string[] {
	if (rect === null) return [];
	const rows = grid.cells
		.slice(rect.top + 1, rect.bottom)
		.map((row) => row.slice(rect.left + 1, rect.right));
	const raw = rows.map((row) =>
		row
			.map(cellText)
			.join('')
			.replace(/[┃│]/gu, ' '),
	);
	const lines = raw.map(normalize);
	const failures: string[] = [];
	if (checkpoint === 'component-quota-floor-draft') {
		const floor = rows.filter((row) => normalize(row.map(cellText).join('')).startsWith('floor '));
		const selected = floor.map((row) =>
			normalize(
				row
					.filter((cell) => cell.fgRgb === terminalTheme.palette[11])
					.map(cellText)
					.join(''),
			).replace(/^[›❯]\s*/u, ''),
		);
		if (floor.length !== 1 || selected[0] !== '1%') failures.push('unique selected Floor value 1%');
	}
	if (checkpoint === 'component-quota-confirm-help-identity') {
		const accountRows = lines
			.map((line, index) => (/^account(?:$|\s)/u.test(line) ? index : -1))
			.filter((index) => index >= 0);
		const account = accountRows[0] ?? -1;
		const boundary = lines.findIndex(
			(line, index) => index > account && (line === 'current quota' || line === 'close'),
		);
		const identity = raw
			.slice(account, boundary)
			.join('')
			.trim()
			.replace(/^Account\s*/u, '')
			.replace(/\s+/gu, '');
		const name = '東京OpenAIResearchAccountAlphaLongName';
		const tag = '[alpha-tag]';
		if (
			accountRows.length !== 1 ||
			account < 0 ||
			boundary <= account ||
			identity.length !== name.length + tag.length ||
			normalize(identity.slice(0, name.length)) !== normalize(name) ||
			identity.slice(name.length) !== tag
		)
			failures.push('exact contextual Account name and case-sensitive entire tag');
	}
	if (
		checkpoint === 'component-quota-confirm-help-top' &&
		!lines.join(' ').includes('selected account account')
	)
		failures.push('Selected account Account within contextual Help');
	if (
		[
			'component-quota-confirm-no',
			'component-quota-confirm-yes',
			'component-quota-confirm-help-returned',
		].includes(checkpoint)
	) {
		const decision = lines.findIndex((line) => line.startsWith('reset quota?'));
		const context = raw.slice(0, decision).join(' ');
		if (
			decision < 0 ||
			!context.includes('[alpha-tag]') ||
			!normalize(context).includes('1 credit. cannot undo.')
		)
			failures.push('decision identity and risk above choices');
		for (const choice of ['No', 'Yes']) {
			const count = raw.join(' ').match(new RegExp('\\b' + choice + '\\b', 'gu'))?.length ?? 0;
			if (count !== 1) failures.push('single decision ' + choice);
		}
		const selected = normalize(
			rows
				.flatMap((row) =>
					row.filter((cell) => cell.fgRgb === terminalTheme.palette[11]).map(cellText),
				)
				.join(''),
		);
		const expected = checkpoint.endsWith('-no') ? 'no' : 'yes';
		if (selected !== expected) failures.push('selected decision ' + expected);
	}
	return failures;
}
