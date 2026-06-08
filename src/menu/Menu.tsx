// Pre-match menu stage machine. Navigation between title/skirmish/multiplayer is
// pure client UI state; once an action founds a player row, App swaps the menu
// for the HUD (server state drives the phase).
import { useState } from 'react';
import type { GameActions } from '../session/useGameActions';
import { MainMenu } from './MainMenu';
import { SkirmishSetup } from './SkirmishSetup';
import { JoinScreen } from './JoinScreen';

type Stage = 'title' | 'skirmish' | 'multiplayer';

export function Menu({
  actions,
  ready,
}: {
  actions: GameActions;
  ready: boolean;
}) {
  const [stage, setStage] = useState<Stage>('title');

  if (stage === 'skirmish')
    return (
      <SkirmishSetup
        disabled={!ready}
        onBack={() => setStage('title')}
        onBegin={actions.startSkirmish}
      />
    );

  if (stage === 'multiplayer')
    return (
      <JoinScreen
        disabled={!ready}
        onBack={() => setStage('title')}
        onJoin={actions.joinMultiplayer}
      />
    );

  return (
    <MainMenu
      disabled={!ready}
      onSkirmish={() => setStage('skirmish')}
      onMultiplayer={() => setStage('multiplayer')}
    />
  );
}
