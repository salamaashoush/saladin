// Menu phase: list the caller's saves with Resume / Delete. The list comes from
// useSaves (the save_slot subscription cache, row-filtered to this player). A
// Resume loads the save into a fresh match; App then swaps to the HUD once the
// loaded player row arrives.
import { useSaves } from "../session/useSaves";
import styles from "./Menu.module.css";

export function SaveLoadScreen({
  onBack,
  onResume,
  onDelete,
  disabled,
}: {
  onBack: () => void;
  onResume: (saveId: string) => void;
  onDelete: (saveId: string) => void;
  disabled: boolean;
}) {
  const saves = useSaves();

  return (
    <div className={styles.screen}>
      <div className={styles.panel}>
        <h2 className={styles.panelTitle}>Saved Campaigns</h2>

        {saves.length === 0 ? (
          <p className={styles.hint}>
            No saves yet. Save a match from the field with “Save &amp; Quit”.
          </p>
        ) : (
          <div className={styles.saveList}>
            {saves.map((s) => (
              <div key={s.id} className={styles.saveRow}>
                <div className={styles.saveInfo}>
                  <span className={styles.saveName}>{s.name}</span>
                  <span className={styles.saveDate}>
                    {new Date(s.createdAt).toLocaleString()}
                  </span>
                </div>
                <div className={styles.saveActions}>
                  <button
                    className={styles.primary}
                    disabled={disabled}
                    onClick={() => onResume(s.id)}
                  >
                    ▶ Resume
                  </button>
                  <button
                    className={styles.removeOpp}
                    title="Delete save"
                    onClick={() => onDelete(s.id)}
                  >
                    ✕
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
