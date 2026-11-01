/** Split product/driver source pins for the authored-flow executor; a plan never grants a run. */
import { z } from 'zod';

import { artifactBindingSchema, sourceBindingSchema } from '../../evidence/evidence-schema.ts';
import { inputConsumptionExpectationSchema } from '../../sessions/input-consumption-schema.ts';
import { journeyGridSizes } from '../journey-definition.ts';

/** Mouse cells are measured from the prior marked frame, never invented before launch. */
export const paintedMouseExpectationSchema = inputConsumptionExpectationSchema
	.unwrap()
	.omit({ event: true })
	.extend({
		kind: z.literal('painted-mouse'),
		label: z.string().min(1),
		mouse: z.enum(['down', 'up']),
		button: z.literal('left'),
		modifiers: z.tuple([]),
		coordinateRule: z.literal('unique-fully-painted-label-in-active-surface'),
	});

export const authoredFlowPlanSchema = z
	.strictObject({
		productRepository: z.string().min(1),
		productSource: sourceBindingSchema,
		productSourceDigest: z.string().regex(/^[a-f0-9]{64}$/u),
		observerRepository: z.string().min(1),
		observerSource: sourceBindingSchema,
		observerSourceDigest: z.string().regex(/^[a-f0-9]{64}$/u),
		driver: artifactBindingSchema,
		product: artifactBindingSchema,
		node: artifactBindingSchema,
		chrome: z.string().min(1),
		output: z.string().min(1),
		journey: z.string().min(1),
		grids: z.array(z.enum(journeyGridSizes)).min(1),
		keys: z.array(z.enum(['legacy', 'kitty'])).min(1),
		expectationPreflight: z
			.strictObject({
				sources: z
					.array(
						z.strictObject({
							path: z.string().min(1),
							sha256: z.string().regex(/^[a-f0-9]{64}$/u),
							rules: z.array(z.string().min(1)).min(1),
						}),
					)
					.min(1),
				cases: z
					.array(
						z.strictObject({
							grid: z.enum(journeyGridSizes),
							keys: z.enum(['legacy', 'kitty']),
							keyboardFlags: z.number().int().nonnegative(),
							inputs: z
								.array(
									z.strictObject({
										operation: z.string().min(1),
										expected: z
											.array(
												z.union([inputConsumptionExpectationSchema, paintedMouseExpectationSchema]),
											)
											.min(1),
										minimumRepeats: z.number().int().nonnegative(),
										maximumRepeats: z.number().int().positive(),
										condition: z.string().min(1),
										sources: z.array(z.string().min(1)).min(1),
									}),
								)
								.min(1),
						}),
					)
					.min(1),
				retainedComparisons: z.array(
					z.strictObject({
						path: z.string().min(1),
						sha256: z.string().regex(/^[a-f0-9]{64}$/u),
						comparedReceipts: z.number().int().nonnegative(),
						result: z.literal('all-reached-fields-match'),
						unreached: z.array(z.string()),
					}),
				),
			})
			.optional(),
	})
	.refine(
		(plan) =>
			new Set(plan.grids).size === plan.grids.length &&
			new Set(plan.keys).size === plan.keys.length,
		'duplicate matrix case',
	);
export type AuthoredFlowPlan = z.infer<typeof authoredFlowPlanSchema>;
