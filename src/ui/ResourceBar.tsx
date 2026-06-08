import { RESOURCE_DEFS, ResourceType } from '../../shared/index.ts';
import { Panel } from './components/Panel';
import styles from './ResourceBar.module.css';

interface ResourceBarProps {
  name: string;
  faction: string;
  wood: number;
  stone: number;
  food: number;
  gold: number;
  starving: boolean;
  peasants: number;
  soldiers: number;
  pop: number;
  cap: number;
}

function Stat({
  icon,
  value,
  label,
  warn,
}: {
  icon: string;
  value: number;
  label: string;
  warn?: boolean;
}) {
  return (
    <div className={`${styles.stat} ${warn ? styles.warn : ''}`}>
      <span className={styles.icon}>{icon}</span>
      <span className={styles.val} style={warn ? { color: '#e06a4a' } : undefined}>
        {value}
      </span>
      <span className={styles.label}>{label}</span>
    </div>
  );
}

export function ResourceBar({
  name,
  faction,
  wood,
  stone,
  food,
  gold,
  starving,
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
        <Stat icon={RESOURCE_DEFS[ResourceType.Wood].icon} value={wood} label="Wood" />
        <Stat icon={RESOURCE_DEFS[ResourceType.Stone].icon} value={stone} label="Stone" />
        <Stat
          icon={RESOURCE_DEFS[ResourceType.Food].icon}
          value={food}
          label={starving ? 'Starving' : 'Food'}
          warn={starving}
        />
        <Stat icon={RESOURCE_DEFS[ResourceType.Gold].icon} value={gold} label="Gold" />
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
