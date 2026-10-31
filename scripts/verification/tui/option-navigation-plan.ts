/** A caller-declared S6b source snapshot and signed artifacts; a plan is not a PTY grant. */
import { z } from 'zod';

import { artifactBindingSchema, sourceBindingSchema } from './evidence-schema.ts';
import { s9GridSizes } from './s9-journey-definition.ts';

export const optionNavigationPlanSchema = z
	.strictObject({
		repository: z.string().min(1),
		observer: z.strictObject({ repository: z.string().min(1), source: sourceBindingSchema }),
		source: sourceBindingSchema,
		requiredCrateHead: z.literal('76d02263fc9e3632839a725442b893ec5dcd77cf'),
		driver: artifactBindingSchema,
		product: artifactBindingSchema,
		node: artifactBindingSchema,
		chrome: z.string().min(1),
		output: z.string().min(1),
		rows: z
			.array(z.strictObject({ label: z.string().min(1), visiblePrefix: z.string().min(1) }))
			.min(3),
		grids: z
			.array(z.enum(s9GridSizes))
			.min(1)
			.default([...s9GridSizes]),
		keys: z
			.array(z.enum(['legacy', 'kitty']))
			.min(1)
			.default(['legacy', 'kitty']),
	})
	.refine(
		(plan) => new Set(plan.rows.map((row) => row.visiblePrefix)).size === plan.rows.length,
		'fixture visible prefixes must identify distinct rows',
	);
export type OptionNavigationPlan = z.infer<typeof optionNavigationPlanSchema>;
