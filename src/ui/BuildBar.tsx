import { useState } from 'react';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  BUILD_CATEGORIES,
  BuildingKind,
  UnitKind,
} from '../../shared/index.ts';
import { useGameStore } from '../store/gameStore';
import styles from './BuildBar.module.css';

interface BuildBarProps {
  wood: number;
  onTrain: (buildingId: string, kind: number) => void;
  onDemolish: (id: string) => void;
  onGatherAll: () => void;
}

const BUILD_ICONS: Record<number, string> = {
  [BuildingKind.Wall]: '🧱',
  [BuildingKind.Gatehouse]: '🚪',
  [BuildingKind.Tower]: '🗼',
  [BuildingKind.House]: '🏠',
  [BuildingKind.Barracks]: '🏛️',
};

const UNIT_ICONS: Record<number, string> = {
  [UnitKind.Peasant]: '🧑‍🌾',
  [UnitKind.Spearman]: '🛡️',
  [UnitKind.Archer]: '🏹',
  [UnitKind.Knight]: '🐎',
};

interface ToolProps {
  icon: string;
  label: string;
  cost?: number;
  active?: boolean;
  disabled?: boolean;
  cls?: string;
  onClick: () => void;
}

function Tool({ icon, label, cost, active, disabled, cls, onClick }: ToolProps) {
  return (
    <button
      type="button"
      title={cost !== undefined ? `${label} — ${cost} wood` : label}
      disabled={disabled}
      onClick={onClick}
      className={`${styles.tool} ${active ? styles.toolActive : ''} ${cls ?? ''}`}
    >
      <span className={styles.toolIcon}>{icon}</span>
      <span className={styles.toolLabel}>{label}</span>
      {cost !== undefined && <span className={styles.toolCost}>{cost}🪵</span>}
    </button>
  );
}

export function BuildBar({
  wood,
  onTrain,
  onDemolish,
  onGatherAll,
}: BuildBarProps) {
  const selB = useGameStore((s) => s.selectedBuilding);
  const buildMode = useGameStore((s) => s.buildMode);
  const setBuildMode = useGameStore((s) => s.setBuildMode);
  const demolishMode = useGameStore((s) => s.demolishMode);
  const setDemolishMode = useGameStore((s) => s.setDemolishMode);
  const [tab, setTab] = useState(0);

  // The "Orders" group is global (always available).
  const orders = (
    <div className={styles.group}>
      <div className={styles.groupLabel}>Orders</div>
      <div className={styles.tools}>
        <Tool icon="🪓" label="Gather" cls={styles.green} onClick={onGatherAll} />
        <Tool
          icon="⛏️"
          label="Demolish"
          active={demolishMode}
          cls={demolishMode ? styles.redActive : styles.red}
          onClick={() => setDemolishMode(!demolishMode)}
        />
      </div>
    </div>
  );

  // Selected building -> production group + orders.
  if (selB) {
    const bdef = BUILDING_DEFS[selB.kind as 0];
    return (
      <div className={styles.bar}>
        <div className={styles.group}>
          <div className={styles.groupLabel}>🏰 {bdef.label}</div>
          <div className={styles.tools}>
            {bdef.trains.length === 0 && (
              <span className={styles.note}>No production</span>
            )}
            {bdef.trains.map((kind) => {
              const u = UNIT_DEFS[kind as 0];
              return (
                <Tool
                  key={kind}
                  icon={UNIT_ICONS[kind]}
                  label={u.label}
                  cost={u.cost}
                  disabled={wood < u.cost}
                  onClick={() => onTrain(selB.id, kind)}
                />
              );
            })}
            {bdef.buildable && (
              <Tool
                icon="⛏️"
                label="Demolish"
                cls={styles.red}
                onClick={() => onDemolish(selB.id)}
              />
            )}
          </div>
        </div>
        {orders}
      </div>
    );
  }

  // Nothing selected -> Build group (tabs + tools) + Orders group.
  const cat = BUILD_CATEGORIES[tab];
  return (
    <div className={styles.bar}>
      <div className={styles.group}>
        <div className={styles.tabs}>
          {BUILD_CATEGORIES.map((c, i) => (
            <button
              key={c.label}
              type="button"
              className={`${styles.tab} ${tab === i ? styles.tabActive : ''}`}
              onClick={() => setTab(i)}
            >
              {c.icon} {c.label}
            </button>
          ))}
        </div>
        <div className={styles.tools}>
          {cat.kinds.map((kind) => {
            const d = BUILDING_DEFS[kind as 0];
            const active = buildMode === kind;
            return (
              <Tool
                key={kind}
                icon={BUILD_ICONS[kind]}
                label={d.label}
                cost={d.cost}
                active={active}
                disabled={!active && wood < d.cost}
                onClick={() => setBuildMode(active ? null : kind)}
              />
            );
          })}
        </div>
      </div>
      {orders}
    </div>
  );
}
