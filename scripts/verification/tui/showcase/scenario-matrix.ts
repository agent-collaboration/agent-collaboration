import type { ScenarioId, ShowcasePlan } from './contracts.ts';
import { variantsForScenario, variantLabel } from './scenario-variants.ts';
import type { ScenarioVariant } from './scenario-variants.ts';
export interface ScenarioMatrixEntry {
	readonly ordinal: number;
	readonly scenarioId: ScenarioId;
	readonly keys: 'legacy' | 'kitty';
	readonly columns: number;
	readonly rows: number;
	readonly variant: ScenarioVariant;
}
export function scenarioMatrix(plan: ShowcasePlan): readonly ScenarioMatrixEntry[] {
	const expanded = plan.cases.flatMap((scenarioId) =>
		plan.keys.flatMap((keys) =>
			plan.grids.flatMap((grid) =>
				variantsForScenario(scenarioId).map((variant) => ({
					scenarioId,
					keys,
					columns: grid.columns,
					rows: grid.rows,
					variant,
				})),
			),
		),
	);
	if (
		plan.variantLabels?.some(
			(label) => !expanded.some((entry) => variantLabel(entry.variant) === label),
		)
	)
		throw new Error('variant label does not belong to the declared cases');
	return expanded
		.filter(
			(entry) =>
				plan.variantLabels === undefined ||
				plan.variantLabels.includes(variantLabel(entry.variant)),
		)
		.map((entry, ordinal) => ({
			ordinal,
			scenarioId: entry.scenarioId,
			keys: entry.keys,
			columns: entry.columns,
			rows: entry.rows,
			variant: entry.variant,
		}));
}
