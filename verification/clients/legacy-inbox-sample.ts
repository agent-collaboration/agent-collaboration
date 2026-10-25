import { createHash } from "node:crypto";
import type { Identity } from "../declaration.ts";
import { encode } from "../exact-json.ts";
import { ObservationOpen } from "../receipt.ts";
import type { LegacyOracle } from "./legacy-oracle.ts";

export interface OracleReaders {
  readonly oracle: LegacyOracle;
  readonly readers: ReadonlyMap<string, Identity>;
}
interface RankedSelection {
  readonly source: OracleReaders;
  readonly selection: LegacyOracle["selections"][number];
  readonly strata: readonly string[];
  readonly rank: string;
}
export function sampleLegacyInbox(input: {
  sources: readonly OracleReaders[];
  seed: string;
  maximumSelections: number;
}): {
  readonly sources: readonly OracleReaders[];
  readonly evidence: {
    readonly seed: string;
    readonly population: number;
    readonly selected: number;
    readonly unsampled: number;
    readonly sourceReaders: number;
    readonly projects: number;
    readonly modes: number;
    readonly coveredStrata: readonly string[];
    readonly selections: readonly {
      readonly sourceSHA256: string;
      readonly reader: string;
      readonly project: string;
      readonly mode: "latest" | "unread";
    }[];
  };
} {
  const population = input.sources.flatMap((source): RankedSelection[] =>
    source.oracle.selections.map((selection): RankedSelection => {
      if (!source.readers.has(selection.reader))
        throw new Error("legacy sample reader has no source identity");
      const key = encode([
        source.oracle.source_sha256,
        selection.reader,
        selection.legacy_topic,
        selection.mode,
      ]);
      return {
        source,
        selection,
        strata: [
          encode(["reader", source.oracle.source_sha256, selection.reader]),
          encode(["project", selection.legacy_topic]),
          encode(["mode", selection.mode]),
        ],
        rank: createHash("sha256")
          .update(encode([input.seed, key]))
          .digest("hex"),
      };
    }),
  );
  population.sort((left, right): number => left.rank.localeCompare(right.rank));
  const uncovered = new Set(population.flatMap((row): readonly string[] => row.strata));
  const allStrata = [...uncovered].sort();
  const selected = new Set<RankedSelection>();
  // Cover the most remaining strata first; the seeded hash breaks every tie.
  while (uncovered.size > 0) {
    let best: RankedSelection | undefined;
    let bestScore = 0;
    for (const row of population) {
      const score = row.strata.filter((stratum): boolean => uncovered.has(stratum)).length;
      if (score > bestScore) {
        best = row;
        bestScore = score;
      }
    }
    if (best === undefined || selected.size >= input.maximumSelections)
      throw new ObservationOpen("legacy inbox sample cap cannot cover every required stratum");
    selected.add(best);
    for (const stratum of best.strata) uncovered.delete(stratum);
  }
  for (const row of population) {
    if (selected.size >= input.maximumSelections) break;
    selected.add(row);
  }
  if (selected.size === 0) throw new ObservationOpen("legacy inbox sample population is empty");
  return {
    sources: input.sources.map((source): OracleReaders => ({
      ...source,
      oracle: {
        ...source.oracle,
        selections: [...selected]
          .filter((row): boolean => row.source === source)
          .map((row): LegacyOracle["selections"][number] => row.selection),
      },
    })),
    evidence: {
      seed: input.seed,
      population: population.length,
      selected: selected.size,
      unsampled: population.length - selected.size,
      sourceReaders: allStrata.filter((stratum): boolean => stratum.startsWith('["reader",'))
        .length,
      projects: allStrata.filter((stratum): boolean => stratum.startsWith('["project",')).length,
      modes: allStrata.filter((stratum): boolean => stratum.startsWith('["mode",')).length,
      coveredStrata: allStrata,
      selections: [...selected].map((row) => ({
        sourceSHA256: row.source.oracle.source_sha256,
        reader: row.selection.reader,
        project: row.selection.legacy_topic,
        mode: row.selection.mode,
      })),
    },
  };
}
