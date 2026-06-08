import { Panel } from './components/Panel';
import styles from './ResourceBar.module.css';

interface ResourceBarProps {
  name: string;
  faction: string;
  wood: number;
  peasants: number;
  soldiers: number;
  pop: number;
  cap: number;
}

function Stat({
  icon,
  value,
  label,
}: {
  icon: string;
  value: number;
  label: string;
}) {
  return (
    <div className={styles.stat}>
      <span className={styles.icon}>{icon}</span>
      <span className={styles.val}>{value}</span>
      <span className={styles.label}>{label}</span>
    </div>
  );
}

export function ResourceBar({
  name,
  faction,
  wood,
  peasants,
  soldiers,
  pop,
  cap,
}: ResourceBarProps) {
  return (
    <Panel className={styles.bar}>
      <div className={styles.faction}>
        <span className={styles.factionName}>⚔️ {name}</span>
        <span className={styles.factionSub}>{faction}</span>
      </div>
      <div className={styles.stats}>
        <Stat icon="🪵" value={wood} label="Wood" />
        <Stat icon="🧑‍🌾" value={peasants} label="Peasants" />
        <Stat icon="🛡️" value={soldiers} label="Army" />
        <div className={styles.stat}>
          <span className={styles.icon}>👥</span>
          <span
            className={styles.val}
            style={pop >= cap ? { color: '#e06a4a' } : undefined}
          >
            {pop}/{cap}
          </span>
          <span className={styles.label}>Pop</span>
        </div>
      </div>
    </Panel>
  );
}
