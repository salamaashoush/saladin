import { useRef } from "react";
import "./App.css";
import { useGameSession } from "./session/useGameSession";
import { useMatch } from "./session/useMatch";
import { useResearch } from "./session/useResearch";
import { useGameActions } from "./session/useGameActions";
import { useGameStore } from "./store/gameStore";
import type { BuildingKind } from "../shared/index.ts";
import { Menu } from "./menu/Menu";
import { GameOver } from "./menu/GameOver";
import { HUD } from "./ui/HUD";

// Top-level router only. Three phases, driven by server state: not connected →
// status; connected but no player row → Menu; player row exists → HUD (+ GameOver
// overlay once the match resolves). All logic lives in the session hooks.
function App() {
  const viewportRef = useRef<HTMLDivElement>(null);
  const { game, identity, isActive, matchId, ready, inMatch } =
    useGameSession(viewportRef);
  const match = useMatch(identity, matchId);
  const actions = useGameActions();
  const lastSkirmish = useGameStore((s) => s.lastSkirmish);
  const ownedBuildings = useGameStore((s) => s.ownedBuildings);

  // The Blacksmith research view (panel rows + completed techs), folded from the
  // live research table + the player's techMask via the SHARED pure helper.
  const research = useResearch(
    identity,
    match.techMask,
    {
      wood: match.wood,
      stone: match.stone,
      food: match.food,
      gold: match.gold,
    },
    new Set(ownedBuildings) as Set<BuildingKind>,
  );

  const rematch = () =>
    lastSkirmish ? actions.startSkirmish(lastSkirmish) : actions.leaveGame();

  // Save the current match under a player-named slot, then drop back to the menu.
  // The save persists server-side; the player resumes it later from Load Game.
  const saveAndQuit = async () => {
    const name =
      window.prompt("Name this save", match.name || "Campaign")?.trim();
    if (!name) return;
    await actions.saveMatch(name);
    actions.leaveGame();
  };

  return (
    <div className="game-root">
      <div className="viewport" ref={viewportRef} />

      {!isActive && <div className="status">Connecting…</div>}

      {isActive && matchId === null && <Menu actions={actions} ready={ready} />}

      {matchId !== null && (
        <HUD
          connected={isActive}
          ready={inMatch}
          name={match.name}
          faction={match.faction}
          wood={match.wood}
          stone={match.stone}
          food={match.food}
          gold={match.gold}
          starving={match.starving}
          peasants={match.peasants}
          soldiers={match.soldiers}
          pop={match.pop}
          cap={match.cap}
          researchRows={research.rows}
          completedTechs={research.completed}
          techMask={match.techMask}
          onTrain={actions.train}
          onDemolish={actions.demolish}
          onGatherAll={actions.gatherAll}
          onTrade={actions.trade}
          onUngarrison={actions.ungarrison}
          onResearch={actions.research}
          onAddAi={() => actions.addAi(1)}
          onSaveAndQuit={saveAndQuit}
          onLeave={actions.leaveGame}
          onSetStance={(s) => game?.setSelectedStance(s)}
          onMinimapCanvas={(c) => game?.setMinimapCanvas(c)}
          onMinimapClick={(x, y) => game?.focusWorld(x, y)}
        />
      )}

      <GameOver
        outcome={match.outcome}
        onRematch={rematch}
        onMenu={actions.leaveGame}
      />
    </div>
  );
}

export default App;
