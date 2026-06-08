import { Stance, UNIT_DEFS, type UnitKind } from '../../shared/index.ts';
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

  // One row per selected UnitKind, ordered by the roster enum, labelled and
  // iconed straight from UNIT_DEFS — new units appear with no edits here.
  const rows = Object.entries(sel.byKind)
    .map(([kind, count]) => ({ kind: Number(kind) as UnitKind, count }))
    .filter((r) => r.count > 0 && UNIT_DEFS[r.kind])
    .sort((a, b) => a.kind - b.kind);

  return (
    <div className={styles.card}>
      <div className={styles.title}>Selection</div>
      <div className={styles.total}>
        {sel.total} unit{sel.total > 1 ? 's' : ''}
      </div>
      <div className={styles.rows}>
        {rows.map(({ kind, count }) => {
          const def = UNIT_DEFS[kind];
          return (
            <div className={styles.row} key={kind}>
              <span className={styles.k}>
                {def.icon} {def.label}
              </span>
              <span className={styles.v}>{count}</span>
            </div>
          );
        })}
      </div>
      {sel.hasCombat && (
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
