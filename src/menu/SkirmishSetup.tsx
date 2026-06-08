import { useState } from 'react';
import { Faction, MATCH_PRESETS, MAP_PRESETS } from '../../shared/index.ts';
import type { SkirmishConfig } from '../session/types';
import { FactionPicker } from './FactionPicker';
import { OpponentList } from './OpponentList';
import styles from './Menu.module.css';

// Parse a user-typed seed: blank/0 means "random" (server rolls one); digits are
// taken verbatim (clamped to u32) so a player can reproduce a remembered map.
function parseSeed(raw: string): number {
  const n = Number.parseInt(raw.trim(), 10);
  return Number.isFinite(n) && n > 0 ? n >>> 0 : 0;
}

export function SkirmishSetup({
  onBegin,
  onBack,
  disabled,
}: {
  onBegin: (config: SkirmishConfig) => void;
  onBack: () => void;
  disabled: boolean;
}) {
  const [name, setName] = useState('Amir');
  const [faction, setFaction] = useState<number>(Faction.Ayyubid);
  const [enemies, setEnemies] = useState<number[]>(MATCH_PRESETS[0].enemies);
  const [mapPreset, setMapPreset] = useState<string>(MAP_PRESETS[0].id);
  const [seedText, setSeedText] = useState('');

  const activeMap = MAP_PRESETS.find((p) => p.id === mapPreset) ?? MAP_PRESETS[0];

  const rollSeed = () =>
    setSeedText(String(1 + Math.floor(Math.random() * 2_000_000_000)));

  return (
    <div className={styles.screen}>
      <div className={styles.panel}>
        <h2 className={styles.panelTitle}>Skirmish</h2>

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
                  p.id === mapPreset ? styles.presetActive : ''
                }`}
                title={p.description}
                onClick={() => setMapPreset(p.id)}
              >
                {p.label}
              </button>
            ))}
          </div>
          <span className={styles.presetHint}>{activeMap.description}</span>
        </div>

        <div className={styles.section}>
          <span className={styles.fieldLabel}>Map seed</span>
          <div className={styles.seedRow}>
            <input
              className={styles.seedInput}
              value={seedText}
              placeholder="random"
              inputMode="numeric"
              maxLength={10}
              onChange={(e) => setSeedText(e.target.value.replace(/\D/g, ''))}
            />
            <button type="button" className={styles.seedBtn} onClick={rollSeed}>
              🎲 Roll
            </button>
          </div>
        </div>

        <div className={styles.section}>
          <span className={styles.fieldLabel}>Match</span>
          <div className={styles.presets}>
            {MATCH_PRESETS.map((p) => (
              <button
                key={p.id}
                type="button"
                className={styles.preset}
                title={p.description}
                onClick={() => setEnemies([...p.enemies])}
              >
                {p.label}
              </button>
            ))}
          </div>
          <OpponentList value={enemies} onChange={setEnemies} />
        </div>

        <div className={styles.actions}>
          <button className={styles.secondary} onClick={onBack}>
            Back
          </button>
          <button
            className={styles.primary}
            disabled={disabled || enemies.length === 0}
            onClick={() =>
              onBegin({
                name: name.trim() || 'Amir',
                faction,
                enemies,
                seed: parseSeed(seedText),
                preset: mapPreset,
              })
            }
          >
            Begin Battle
          </button>
        </div>
      </div>
    </div>
  );
}
