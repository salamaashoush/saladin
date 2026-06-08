import { useGameStore } from '../store/gameStore';
import styles from './CommandCard.module.css';

export function CommandCard() {
  const sel = useGameStore((s) => s.selection);
  if (sel.total === 0) return null;

  const hpColor =
    sel.avgHp > 0.5 ? '#5b8a3a' : sel.avgHp > 0.25 ? '#c9a227' : '#b6402f';

  const rows: Array<[string, number]> = [
    ['🧑‍🌾 Peasants', sel.peasants],
    ['🛡️ Spearmen', sel.spearmen],
    ['🏹 Archers', sel.archers],
  ].filter(([, n]) => (n as number) > 0) as Array<[string, number]>;

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
      <div className={styles.hpbar}>
        <div
          className={styles.hpfill}
          style={{ width: `${Math.round(sel.avgHp * 100)}%`, background: hpColor }}
        />
      </div>
    </div>
  );
}
