import { useEffect } from 'react';
import { useGameStore } from '../store/gameStore';
import styles from './Toasts.module.css';

export function Toasts() {
  const toasts = useGameStore((s) => s.toasts);
  const dismiss = useGameStore((s) => s.dismissToast);

  useEffect(() => {
    if (toasts.length === 0) return;
    const timers = toasts.map((t) =>
      setTimeout(() => dismiss(t.id), 2600)
    );
    return () => timers.forEach(clearTimeout);
  }, [toasts, dismiss]);

  if (toasts.length === 0) return null;
  return (
    <div className={styles.wrap}>
      {toasts.map((t) => (
        <div key={t.id} className={`${styles.toast} ${styles[t.kind]}`}>
          {t.text}
        </div>
      ))}
    </div>
  );
}
