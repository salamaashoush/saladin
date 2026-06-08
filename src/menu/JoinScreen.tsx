import { useState } from 'react';
import { Faction } from '../../shared/index.ts';
import type { JoinConfig } from '../session/types';
import { FactionPicker } from './FactionPicker';
import styles from './Menu.module.css';

export function JoinScreen({
  onJoin,
  onBack,
  disabled,
}: {
  onJoin: (config: JoinConfig) => void;
  onBack: () => void;
  disabled: boolean;
}) {
  const [name, setName] = useState('Amir');
  const [faction, setFaction] = useState<number>(Faction.Ayyubid);

  return (
    <div className={styles.screen}>
      <div className={styles.panel}>
        <h2 className={styles.panelTitle}>Multiplayer</h2>
        <p className={styles.hint}>
          Join the shared world. Others on this server stand beside or against
          you.
        </p>

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

        <div className={styles.actions}>
          <button className={styles.secondary} onClick={onBack}>
            Back
          </button>
          <button
            className={styles.primary}
            disabled={disabled}
            onClick={() => onJoin({ name: name.trim() || 'Amir', faction })}
          >
            Enter the World
          </button>
        </div>
      </div>
    </div>
  );
}
