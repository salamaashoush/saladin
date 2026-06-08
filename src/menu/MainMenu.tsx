import styles from './Menu.module.css';

export function MainMenu({
  onSkirmish,
  onMultiplayer,
  disabled,
}: {
  onSkirmish: () => void;
  onMultiplayer: () => void;
  disabled: boolean;
}) {
  return (
    <div className={styles.screen}>
      <div className={styles.brand}>
        <h1 className={styles.title}>SALADIN</h1>
        <p className={styles.subtitle}>
          Wars of the Crescent — the Ayyubid campaigns
        </p>
      </div>

      <div className={styles.menuButtons}>
        <button
          className={styles.primary}
          disabled={disabled}
          onClick={onSkirmish}
        >
          ⚔ Skirmish
        </button>
        <button
          className={styles.secondary}
          disabled={disabled}
          onClick={onMultiplayer}
        >
          🌐 Multiplayer
        </button>
        <button className={styles.secondary} disabled title="Coming soon">
          📜 Campaign — soon
        </button>
      </div>

      {disabled && <p className={styles.hint}>Reaching the field…</p>}
    </div>
  );
}
