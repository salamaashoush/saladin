import { useState } from 'react';
import { Faction, MATCH_PRESETS } from '../../shared/index.ts';
import type { SkirmishConfig } from '../session/types';
import { FactionPicker } from './FactionPicker';
import { OpponentList } from './OpponentList';
import styles from './Menu.module.css';

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
              onBegin({ name: name.trim() || 'Amir', faction, enemies })
            }
          >
            Begin Battle
          </button>
        </div>
      </div>
    </div>
  );
}
