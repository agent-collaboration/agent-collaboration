/** Wire projection of InputDispatchReceipt at producer 9bcab4fff; no text payloads. */
import { z } from 'zod';

const modifierNames = ['shift', 'control', 'alt', 'super', 'hyper', 'meta'] as const;
const modifiersSchema = z
	.array(z.enum(modifierNames))
	.refine(
		(values) =>
			values.every(
				(value, index) =>
					index === 0 ||
					modifierNames.indexOf(value) > modifierNames.indexOf(values[index - 1] ?? value),
			),
		'input modifiers must be unique and in producer order',
	)
	.readonly();
const identifierSchema = z.string().refine((value) => value.trim().length > 0);
const overlayIdSchema = z
	.string()
	.regex(/^(0|[1-9][0-9]*)$/u)
	.refine((value) => BigInt(value) <= 18_446_744_073_709_551_615n);
const cellIndexSchema = z.number().int().min(0).max(65_535);
const keyCodeSchema = z
	.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('text') }),
		z.strictObject({
			kind: z.literal('shortcut'),
			key: z.string().regex(/^[\x20-\x40\x5b-\x7e]$/u),
		}),
		z.strictObject({
			kind: z.literal('control'),
			codepoint: z
				.number()
				.int()
				.min(0)
				.max(0x10ffff)
				.refine((value) => /^\p{Cc}$/u.test(String.fromCodePoint(value))),
		}),
		// Fixed names are the underlying typed KeyCode Display, not a wire enum.
		z.strictObject({ kind: z.literal('fixed'), name: z.string().min(1) }),
	])
	.readonly();
const mouseKindSchema = z
	.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('down'), button: z.enum(['left', 'middle', 'right']) }),
		z.strictObject({ kind: z.literal('up'), button: z.enum(['left', 'middle', 'right']) }),
		z.strictObject({ kind: z.literal('drag'), button: z.enum(['left', 'middle', 'right']) }),
		z.strictObject({ kind: z.literal('moved') }),
		z.strictObject({ kind: z.literal('scrollDown') }),
		z.strictObject({ kind: z.literal('scrollUp') }),
		z.strictObject({ kind: z.literal('scrollLeft') }),
		z.strictObject({ kind: z.literal('scrollRight') }),
	])
	.readonly();
export const inputEventReadbackSchema = z
	.discriminatedUnion('kind', [
		z
			.strictObject({
				kind: z.literal('key'),
				code: keyCodeSchema,
				modifiers: modifiersSchema,
				phase: z.enum(['press', 'repeat', 'release']),
			})
			.refine((event) => event.code.kind !== 'shortcut' || event.modifiers.includes('control')),
		z.strictObject({
			kind: z.literal('mouse'),
			mouse: mouseKindSchema,
			column: cellIndexSchema,
			row: cellIndexSchema,
			modifiers: modifiersSchema,
		}),
		z.strictObject({ kind: z.literal('resize'), columns: cellIndexSchema, rows: cellIndexSchema }),
		z.strictObject({ kind: z.literal('other'), event: z.enum(['bracketedPaste', 'unknown']) }),
	])
	.readonly();
export const inputSurfaceReadbackSchema = z
	.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('screen') }),
		z.strictObject({ kind: z.literal('overlay'), overlayId: overlayIdSchema }),
	])
	.readonly();
const receiptFieldsSchema = z.strictObject({
	sequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	event: inputEventReadbackSchema,
	surface: inputSurfaceReadbackSchema,
	target: identifierSchema.nullable(),
	action: identifierSchema.nullable(),
});
export const inputDispatchReceiptSchema = receiptFieldsSchema.readonly();
export const inputConsumptionExpectationSchema = receiptFieldsSchema
	.omit({ sequence: true })
	.readonly();
export type InputEventReadback = z.infer<typeof inputEventReadbackSchema>;
export type InputDispatchReceipt = z.infer<typeof inputDispatchReceiptSchema>;
export type InputConsumptionExpectation = z.infer<typeof inputConsumptionExpectationSchema>;
