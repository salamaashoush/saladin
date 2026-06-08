// Multiplayer lobby: list the open matches on this server and let the player join
// one, or host a new one. The list IS the `match` subscription cache (useLobby) —
// it streams live, so a row appears/updates the instant a match opens or fills,
// with no client-held mirror and no manual refresh.
//
// Join calls enterGame(matchId, name, faction); Create calls createMatch(name,
// faction, preset). App swaps the menu for the HUD the moment the founded/joined
// player row arrives (matchId becomes non-null), so this screen never has to route
// into the game itself.
import { useState } from "react";
import { Faction, MAP_PRESETS } from "../../shared/index.ts";
import { useLobby } from "../session/useLobby";
import type { JoinConfig, CreateMatchConfig } from "../session/types";
import { FactionPicker } from "./FactionPicker";
import styles from "./Menu.module.css";

export function Lobby({
  onJoin,
  onCreate,
  onBack,
  disabled,
}: {
  onJoin: (config: JoinConfig) => void;
  onCreate: (config: CreateMatchConfig) => void;
  onBack: () => void;
  disabled: boolean;
}) {
  const matches = useLobby();
  const [name, setName] = useState("Amir");
  const [faction, setFaction] = useState<number>(Faction.Ayyubid);
  const [preset, setPreset] = useState<string>(MAP_PRESETS[0].id);
  const [creating, setCreating] = useState(false);

  if (creating) {
    return (
      <div className={styles.screen}>
        <div className={styles.panel}>
          <h2 className={styles.panelTitle}>Host a Match</h2>

          <label className={styles.field}>
            <span className={styles.fieldLabel}>Commander</span>
            <input
              className={styles.input}
              value={name}
              maxLength={24}
              onChange={(e) => setName(e.target.value)}
            />
          </label>

          <div className={styles.section}>
            <span className={styles.fieldLabel}>Your faction</span>
            <FactionPicker value={faction} onChange={setFaction} />
          </div>

          <div className={styles.section}>
            <span className={styles.fieldLabel}>Map</span>
            <div className={styles.presets}>
              {MAP_PRESETS.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  className={`${styles.preset} ${
                    p.id === preset ? styles.presetActive : ""
                  }`}
                  title={p.description}
                  onClick={() => setPreset(p.id)}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>

          <div className={styles.actions}>
            <button
              className={styles.secondary}
              onClick={() => setCreating(false)}
            >
              ← Back
            </button>
            <button
              className={styles.primary}
              disabled={disabled}
              onClick={() =>
                onCreate({ name: name.trim() || "Amir", faction, preset })
              }
            >
              Create &amp; Host
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.screen}>
      <div className={styles.panel}>
        <div className={styles.lobbyHeader}>
          <h2 className={styles.panelTitle}>Open Matches</h2>
          <button
            className={styles.primary}
            disabled={disabled}
            onClick={() => setCreating(true)}
          >
            + Create Match
          </button>
        </div>

        <label className={styles.field}>
          <span className={styles.fieldLabel}>Commander</span>
          <input
            className={styles.input}
            value={name}
            maxLength={24}
            onChange={(e) => setName(e.target.value)}
          />
        </label>

        <div className={styles.section}>
          <span className={styles.fieldLabel}>Your faction</span>
          <FactionPicker value={faction} onChange={setFaction} />
        </div>

        {matches.length === 0 ? (
          <p className={styles.hint}>
            No open matches. Host one with “+ Create Match” and wait for others
            to join.
          </p>
        ) : (
          <div className={styles.saveList}>
            {matches.map((m) => (
              <div key={m.id} className={styles.saveRow}>
                <div className={styles.saveInfo}>
                  <span className={styles.saveName}>{m.name}</span>
                  <div className={styles.lobbyMeta}>
                    <span className={styles.lobbyTag}>{m.presetLabel}</span>
                  </div>
                </div>
                <div className={styles.saveActions}>
                  <span className={styles.playerBadge}>
                    {m.players}/{m.maxPlayers}
                  </span>
                  <button
                    className={styles.primary}
                    disabled={disabled}
                    onClick={() =>
                      onJoin({
                        matchId: m.id,
                        name: name.trim() || "Amir",
                        faction,
                      })
                    }
                  >
                    Join
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <div className={styles.actions}>
          <button className={styles.secondary} onClick={onBack}>
            ← Back
          </button>
        </div>
      </div>
    </div>
  );
}
