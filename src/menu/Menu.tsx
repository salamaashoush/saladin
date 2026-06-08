// Pre-match menu stage machine. Navigation between title/skirmish/multiplayer is
// pure client UI state; once an action founds a player row, App swaps the menu
// for the HUD (server state drives the phase).
import { useState } from "react";
import type { GameActions } from "../session/useGameActions";
import { MainMenu } from "./MainMenu";
import { SkirmishSetup } from "./SkirmishSetup";
import { Lobby } from "./Lobby";
import { SaveLoadScreen } from "./SaveLoadScreen";

type Stage = "title" | "skirmish" | "multiplayer" | "load";

export function Menu({
  actions,
  ready,
}: {
  actions: GameActions;
  ready: boolean;
}) {
  const [stage, setStage] = useState<Stage>("title");

  if (stage === "skirmish")
    return (
      <SkirmishSetup
        disabled={!ready}
        onBack={() => setStage("title")}
        onBegin={actions.startSkirmish}
      />
    );

  if (stage === "multiplayer")
    return (
      <Lobby
        disabled={!ready}
        onBack={() => setStage("title")}
        onJoin={actions.join}
        onCreate={actions.createMatch}
      />
    );

  if (stage === "load")
    return (
      <SaveLoadScreen
        disabled={!ready}
        onBack={() => setStage("title")}
        onResume={actions.loadMatch}
        onDelete={actions.deleteSave}
      />
    );

  return (
    <MainMenu
      disabled={!ready}
      onSkirmish={() => setStage("skirmish")}
      onMultiplayer={() => setStage("multiplayer")}
      onLoad={() => setStage("load")}
    />
  );
}
