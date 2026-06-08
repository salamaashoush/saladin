import { useRef } from 'react';
import './App.css';
import { useGameSession } from './session/useGameSession';
import { useMatch } from './session/useMatch';
import { useGameActions } from './session/useGameActions';
import { useGameStore } from './store/gameStore';
import { Menu } from './menu/Menu';
import { GameOver } from './menu/GameOver';
import { HUD } from './ui/HUD';

// Top-level router only. Three phases, driven by server state: not connected →
// status; connected but no player row → Menu; player row exists → HUD (+ GameOver
// overlay once the match resolves). All logic lives in the session hooks.
function App() {
  const viewportRef = useRef<HTMLDivElement>(null);
  const { game, identity, isActive, ready } = useGameSession(viewportRef);
  const match = useMatch(identity);
  const actions = useGameActions();
  const lastSkirmish = useGameStore((s) => s.lastSkirmish);

  const rematch = () =>
    lastSkirmish ? actions.startSkirmish(lastSkirmish) : actions.leaveGame();

  return (
    <div className="game-root">
      <div className="viewport" ref={viewportRef} />

      {!isActive && <div className="status">Connecting…</div>}

      {isActive && !match.inGame && <Menu actions={actions} ready={ready} />}

      {match.inGame && (
        <HUD
          connected={isActive}
          ready={ready}
          name={match.name}
          faction={match.faction}
          wood={match.wood}
          peasants={match.peasants}
          soldiers={match.soldiers}
          pop={match.pop}
          cap={match.cap}
          onTrain={actions.train}
          onDemolish={actions.demolish}
          onGatherAll={actions.gatherAll}
          onAddAi={() => actions.addAi(1)}
          onLeave={actions.leaveGame}
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
