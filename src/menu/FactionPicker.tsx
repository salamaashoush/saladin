import { Faction, FACTION_LABELS } from '../../shared/index.ts';
import styles from './Menu.module.css';

const INFO: Record<number, { emblem: string; blurb: string }> = {
  [Faction.Ayyubid]: {
    emblem: '☪️',
    blurb: 'Salah ad-Din’s host — swift cavalry and archers.',
  },
  [Faction.Crusader]: {
    emblem: '✝️',
    blurb: 'The Crusader states — heavy infantry and stone.',
  },
};

export function FactionPicker({
  value,
  onChange,
}: {
  value: number;
  onChange: (faction: number) => void;
}) {
  return (
    <div className={styles.factions}>
      {[Faction.Ayyubid, Faction.Crusader].map((f) => (
        <button
          key={f}
          type="button"
          className={`${styles.factionCard} ${value === f ? styles.factionActive : ''}`}
          onClick={() => onChange(f)}
        >
          <span className={styles.factionEmblem}>{INFO[f].emblem}</span>
          <span className={styles.factionLabel}>{FACTION_LABELS[f]}</span>
          <span className={styles.factionBlurb}>{INFO[f].blurb}</span>
        </button>
      ))}
    </div>
  );
}
