import {
  AI_PROFILES,
  AiDifficulty,
  MAX_AI_OPPONENTS,
} from '../../shared/index.ts';
import styles from './Menu.module.css';

const DIFFICULTIES = [
  AiDifficulty.Easy,
  AiDifficulty.Normal,
  AiDifficulty.Hard,
];

export function OpponentList({
  value,
  onChange,
}: {
  value: number[];
  onChange: (opponents: number[]) => void;
}) {
  const setDiff = (i: number, d: number) =>
    onChange(value.map((v, j) => (j === i ? d : v)));
  const add = () =>
    value.length < MAX_AI_OPPONENTS &&
    onChange([...value, AiDifficulty.Normal]);
  const remove = (i: number) => onChange(value.filter((_, j) => j !== i));

  return (
    <div className={styles.opponents}>
      {value.map((d, i) => (
        <div className={styles.opponent} key={i}>
          <span className={styles.opponentName}>Rival {i + 1}</span>
          <div className={styles.diffs}>
            {DIFFICULTIES.map((dd) => (
              <button
                key={dd}
                type="button"
                className={`${styles.diff} ${d === dd ? styles.diffActive : ''}`}
                onClick={() => setDiff(i, dd)}
              >
                {AI_PROFILES[dd].label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className={styles.removeOpp}
            title="Remove rival"
            onClick={() => remove(i)}
          >
            ✕
          </button>
        </div>
      ))}

      {value.length < MAX_AI_OPPONENTS && (
        <button type="button" className={styles.addOpp} onClick={add}>
          + Add rival
        </button>
      )}
      {value.length === 0 && (
        <p className={styles.hint}>Add at least one rival to begin.</p>
      )}
    </div>
  );
}
