/** Opt-in input transport for existing journeys; PTY ownership remains in PtyGridDriver. */
import { applyExampleGhosttyKeybinds } from './example-ghostty-keybinds.ts';
import type { GhosttyInputProfile } from './example-ghostty-keybinds.ts';
import {
	encodeGhosttyKey,
	encodeGhosttyMouse,
	encodeGhosttyText,
	ghosttyKeySchema,
} from './ghostty-input-encoding.ts';
import type { CellMouseEvent, TerminalInputModes } from './ghostty-input-encoding.ts';

export interface GhosttyInputTransport {
	inputModes(): TerminalInputModes;
	send(bytes: Uint8Array): Promise<void>;
}
export type InputDeliveryRecord = {
	readonly input:
		| { readonly kind: 'key'; readonly key: string }
		| { readonly kind: 'text'; readonly text: string }
		| CellMouseEvent;
	readonly modes: TerminalInputModes;
	readonly profile: GhosttyInputProfile;
	readonly delivery:
		| {
				readonly kind: 'sent';
				readonly route: 'encoder' | 'keybind-replacement' | 'sgr1006';
				readonly hex: string;
		  }
		| { readonly kind: 'consumed' | 'unsupported'; readonly reason: string };
};
export class GhosttyJourneyInput {
	private readonly records: InputDeliveryRecord[] = [];
	private readonly transport: GhosttyInputTransport;
	private readonly profile: GhosttyInputProfile;
	constructor(transport: GhosttyInputTransport, profile: GhosttyInputProfile) {
		this.transport = transport;
		this.profile = profile;
	}
	history(): readonly InputDeliveryRecord[] {
		return [...this.records];
	}
	async key(value: string): Promise<InputDeliveryRecord> {
		const key = ghosttyKeySchema.parse(value);
		const modes = this.transport.inputModes();
		const binding = applyExampleGhosttyKeybinds(key, this.profile);
		const input = { kind: 'key', key } as const;
		if (binding.kind === 'consumed')
			return this.keep({
				input,
				modes,
				profile: this.profile,
				delivery: { kind: 'consumed', reason: binding.action },
			});
		return this.transmit(
			input,
			modes,
			binding.kind === 'replacement' ? binding.bytes : encodeGhosttyKey(key, modes),
			binding.kind === 'replacement' ? 'keybind-replacement' : 'encoder',
		);
	}
	async text(text: string): Promise<InputDeliveryRecord> {
		const modes = this.transport.inputModes();
		return this.transmit({ kind: 'text', text }, modes, encodeGhosttyText(text, modes), 'encoder');
	}
	async mouse(event: CellMouseEvent): Promise<InputDeliveryRecord> {
		const modes = this.transport.inputModes();
		const encoded = encodeGhosttyMouse(event, modes);
		if (encoded.kind !== 'bytes')
			return this.keep({ input: event, modes, profile: this.profile, delivery: encoded });
		return this.transmit(event, modes, encoded.bytes, 'sgr1006');
	}
	async click(column: number, row: number): Promise<readonly InputDeliveryRecord[]> {
		return [
			await this.mouse({ kind: 'press', button: 'left', column, row }),
			await this.mouse({ kind: 'release', button: 'left', column, row }),
		];
	}
	private keep(record: InputDeliveryRecord): InputDeliveryRecord {
		this.records.push(record);
		return record;
	}
	private async transmit(
		input: InputDeliveryRecord['input'],
		modes: TerminalInputModes,
		bytes: Buffer,
		route: 'encoder' | 'keybind-replacement' | 'sgr1006',
	): Promise<InputDeliveryRecord> {
		await this.transport.send(bytes);
		return this.keep({
			input,
			modes,
			profile: this.profile,
			delivery: { kind: 'sent', route, hex: bytes.toString('hex') },
		});
	}
}
export function requireDelivered(record: InputDeliveryRecord): void {
	if (record.delivery.kind !== 'sent')
		throw new Error(`input prerequisite finding: ${JSON.stringify(record)}`);
}
