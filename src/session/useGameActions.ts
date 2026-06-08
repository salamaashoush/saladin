// The single typed gateway for every reducer call. Each action is guarded so a
// rejected reducer surfaces as a toast instead of an unhandled rejection. UI
// components call these — they never touch `reducers` directly.
import { useReducer } from "spacetimedb/react";
import { reducers } from "../module_bindings";
import { useGameStore } from "../store/gameStore";
import type { SkirmishConfig, JoinConfig, CreateMatchConfig } from "./types";

export function useGameActions() {
  const pushToast = useGameStore((s) => s.pushToast);
  const setLastSkirmish = useGameStore((s) => s.setLastSkirmish);

  const startSkirmish = useReducer(reducers.startSkirmish);
  const enterGame = useReducer(reducers.enterGame);
  const createMatch = useReducer(reducers.createMatch);
  const leaveGame = useReducer(reducers.leaveGame);
  const addAi = useReducer(reducers.addAi);
  const trainUnit = useReducer(reducers.trainUnit);
  const demolishBuilding = useReducer(reducers.demolishBuilding);
  const autoGather = useReducer(reducers.autoGather);
  const marketTrade = useReducer(reducers.marketTrade);
  const garrisonUnit = useReducer(reducers.garrisonUnit);
  const ungarrisonBuilding = useReducer(reducers.ungarrisonBuilding);
  const startResearch = useReducer(reducers.startResearch);
  const saveMatch = useReducer(reducers.saveMatch);
  const loadMatch = useReducer(reducers.loadMatch);
  const deleteSave = useReducer(reducers.deleteSave);

  const guard = (p: unknown) =>
    Promise.resolve(p).catch((e: unknown) =>
      pushToast(e instanceof Error ? e.message : "Action failed", "error"),
    );

  return {
    startSkirmish: (c: SkirmishConfig) => {
      setLastSkirmish(c);
      return guard(
        startSkirmish({
          name: c.name,
          faction: c.faction,
          enemies: new Uint8Array(c.enemies),
          seed: c.seed >>> 0,
          preset: c.preset,
        }),
      );
    },
    // Join an existing open match by its id (the lobby's Join button).
    join: (c: JoinConfig) =>
      guard(
        enterGame({
          matchId: BigInt(c.matchId),
          name: c.name,
          faction: c.faction,
        }),
      ),
    // Host a fresh multiplayer match (the lobby's Create button).
    createMatch: (c: CreateMatchConfig) =>
      guard(
        createMatch({ name: c.name, faction: c.faction, preset: c.preset }),
      ),
    leaveGame: () => guard(leaveGame()),
    addAi: (difficulty: number) => guard(addAi({ difficulty })),
    train: (buildingId: string, kind: number) =>
      guard(trainUnit({ buildingId: BigInt(buildingId), kind })),
    demolish: (id: string) => guard(demolishBuilding({ entityId: BigInt(id) })),
    gatherAll: () => guard(autoGather()),
    trade: (resType: number, amount: number) =>
      guard(marketTrade({ resType, amount })),
    garrison: (unitId: string, buildingId: string) =>
      guard(
        garrisonUnit({
          unitId: BigInt(unitId),
          buildingId: BigInt(buildingId),
        }),
      ),
    ungarrison: (buildingId: string) =>
      guard(ungarrisonBuilding({ buildingId: BigInt(buildingId) })),
    research: (buildingId: string, tech: number) =>
      guard(startResearch({ buildingId: BigInt(buildingId), tech })),
    saveMatch: (name: string) => guard(saveMatch({ name })),
    loadMatch: (saveId: string) => guard(loadMatch({ saveId: BigInt(saveId) })),
    deleteSave: (saveId: string) =>
      guard(deleteSave({ saveId: BigInt(saveId) })),
  };
}

export type GameActions = ReturnType<typeof useGameActions>;
