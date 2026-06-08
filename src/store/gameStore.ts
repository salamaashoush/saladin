// Client-only UI state shared between the Three.js game and the React HUD.
// The game writes (selection, toasts) via getState(); the HUD reads reactively.
// Server state stays in SpacetimeDB tables (useTable), not here.
import { create } from 'zustand';
import type { SkirmishConfig } from '../session/types';

export interface SelectionSummary {
  total: number;
  byKind: Record<number, number>; // UnitKind -> count; new units appear automatically
  hasCombat: boolean; // any selected unit can attack (gates the stance buttons)
  avgHp: number; // 0..1
}

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'error';
}

interface GameUIState {
  selection: SelectionSummary;
  selectedBuilding: { id: string; kind: number } | null;
  ownedBuildings: number[]; // BuildingKinds the player currently owns (tech tree)
  toasts: Toast[];
  buildMode: number | null; // BuildingKind being placed, or null
  demolishMode: boolean;
  lastSkirmish: SkirmishConfig | null; // remembered for "Rematch"
  setSelection: (s: SelectionSummary) => void;
  setSelectedBuilding: (b: { id: string; kind: number } | null) => void;
  setOwnedBuildings: (kinds: number[]) => void;
  pushToast: (text: string, kind?: Toast['kind']) => void;
  dismissToast: (id: number) => void;
  setBuildMode: (kind: number | null) => void;
  setDemolishMode: (on: boolean) => void;
  setLastSkirmish: (c: SkirmishConfig) => void;
}

const EMPTY_SELECTION: SelectionSummary = {
  total: 0,
  byKind: {},
  hasCombat: false,
  avgHp: 1,
};

let nextToastId = 1;

export const useGameStore = create<GameUIState>((set) => ({
  selection: EMPTY_SELECTION,
  selectedBuilding: null,
  ownedBuildings: [],
  toasts: [],
  buildMode: null,
  demolishMode: false,
  lastSkirmish: null,
  setSelection: (selection) => set({ selection }),
  setSelectedBuilding: (selectedBuilding) => set({ selectedBuilding }),
  setOwnedBuildings: (ownedBuildings) => set({ ownedBuildings }),
  pushToast: (text, kind = 'info') =>
    set((s) => ({ toasts: [...s.toasts, { id: nextToastId++, text, kind }] })),
  dismissToast: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  setBuildMode: (buildMode) => set({ buildMode, demolishMode: false }),
  setDemolishMode: (demolishMode) => set({ demolishMode, buildMode: null }),
  setLastSkirmish: (lastSkirmish) => set({ lastSkirmish }),
}));

if (import.meta.env?.DEV)
  (window as unknown as { __store: typeof useGameStore }).__store = useGameStore;
