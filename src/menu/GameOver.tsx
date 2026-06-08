import type { Outcome } from '../session/useMatch';
import styles from './Menu.module.css';

export function GameOver({
  outcome,
  onRematch,
  onMenu,
}: {
  outcome: Outcome;
  onRematch: () => void;
  onMenu: () => void;
}) {
  if (!outcome) return null;
  const won = outcome === 'victory';

  return (
    <div className={styles.overlay}>
      <h1 className={`${styles.outcome} ${won ? styles.victory : styles.defeat}`}>
        {won ? 'Victory' : 'Defeat'}
      </h1>
      <p className={styles.outcomeSub}>
        {won ? 'The enemy keeps have fallen.' : 'Your keep has fallen.'}
      </p>
      <div className={styles.actions}>
        <button className={styles.primary} onClick={onRematch}>
          Rematch
        </button>
        <button className={styles.secondary} onClick={onMenu}>
          Main Menu
        </button>
      </div>
    </div>
  );
}
