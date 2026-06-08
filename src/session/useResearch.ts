// Derives the local player's Blacksmith research view from the live tables. The
// cache IS the source of truth: research rows come straight from useTable and the
// panel state is folded by the SHARED researchPanelState helper, so the UI obeys
// the exact afford/prereq/done rules the module enforces. No local mirrors.
import { useTable } from "spacetimedb/react";
import type { Identity } from "spacetimedb";
import { tables } from "../module_bindings";
import {
  researchPanelState,
  techsInMask,
  UPGRADE_DEFS,
  type ResearchRowState,
  type Stockpile,
  type Tech,
  type BuildingKind,
} from "../../shared/index.ts";

export interface CompletedTech {
  tech: Tech;
  label: string;
  icon: string;
}

export interface ResearchView {
  rows: ResearchRowState[]; // one descriptor per tech (status + progress + cost)
  completed: CompletedTech[]; // owner's finished techs, for the legible tech row
}

export function useResearch(
  identity: Identity | undefined,
  techMask: bigint,
  stock: Stockpile,
  ownedBuildings: ReadonlySet<BuildingKind>,
): ResearchView {
  // research carries no matchId; scope the subscription to the caller (owner) so
  // it stays an index-backed point query and opens nothing at the menu.
  const [mine] = useTable(
    identity
      ? tables.research.where((r) => r.owner.eq(identity))
      : tables.research,
    { enabled: !!identity },
  );

  const rows = researchPanelState(techMask, mine, stock, ownedBuildings);

  const completed: CompletedTech[] = techsInMask(techMask).map((tech) => ({
    tech,
    label: UPGRADE_DEFS[tech].label,
    icon: UPGRADE_DEFS[tech].icon,
  }));

  return { rows, completed };
}
