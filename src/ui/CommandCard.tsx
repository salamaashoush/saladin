import { Stance } from '../../shared/index.ts';
import { useGameStore } from '../store/gameStore';
import styles from './CommandCard.module.css';

const STANCES: Array<[number, string, string]> = [
  [Stance.Aggressive, '⚔️', 'Aggressive'],
  [Stance.Defensive, '🛡️', 'Defensive'],
  [Stance.HoldGround, '⛰️', 'Hold'],
];

export function CommandCard({
  onSetStance,
}: {
  onSetStance: (stance: number) => void;
}) {
  const sel = useGameStore((s) => s.selection);
  if (sel.total === 0) return null;

  const hpColor =
    sel.avgHp > 0.5 ? '#5b8a3a' : sel.avgHp > 0.25 ? '#c9a227' : '#b6402f';

  const rows: Array<[string, number]> = (
    [
      ['🧑‍🌾 Peasants', sel.peasants],
      ['🛡️ Spearmen', sel.spearmen],
      ['🏹 Archers', sel.archers],
      ['🐎 Knights', sel.knights],
    ] as Array<[string, number]>
  ).filter(([, n]) => n > 0);

  const soldiers = sel.spearmen + sel.archers + sel.knights;

  return (
    <div className={styles.card}>
      <div className={styles.title}>Selection</div>
      <div className={styles.total}>
        {sel.total} unit{sel.total > 1 ? 's' : ''}
      </div>
      <div className={styles.rows}>
        {rows.map(([k, v]) => (
          <div className={styles.row} key={k}>
            <span className={styles.k}>{k}</span>
            <span className={styles.v}>{v}</span>
          </div>
        ))}
      </div>
      {soldiers > 0 && (
        <div className={styles.stances}>
          {STANCES.map(([s, icon, label]) => (
            <button
              key={s}
              type="button"
              className={styles.stance}
              title={label}
              onClick={() => onSetStance(s)}
            >
              {icon}
            </button>
          ))}
        </div>
      )}
      <div className={styles.hpbar}>
        <div
          className={styles.hpfill}
          style={{ width: `${Math.round(sel.avgHp * 100)}%`, background: hpColor }}
        />
      </div>
    </div>
  );
}
