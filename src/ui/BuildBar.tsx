import { useState } from 'react';
import {
  UNIT_DEFS,
  BUILDING_DEFS,
  BUILD_CATEGORIES,
  RESOURCE_DEFS,
  ResourceType,
  BuildingKind,
  canAfford,
  hasPrereq,
  type ResourceCost,
} from '../../shared/index.ts';
import { useGameStore } from '../store/gameStore';
import styles from './BuildBar.module.css';

interface BuildBarProps {
  wood: number;
  stone: number;
  food: number;
  gold: number;
  onTrain: (buildingId: string, kind: number) => void;
  onDemolish: (id: string) => void;
  onGatherAll: () => void;
  onTrade: (resType: number, amount: number) => void;
}

const MARKET_LOT = 20; // resources sold per market click

interface ToolProps {
  icon: string;
  label: string;
  cost?: ResourceCost;
  active?: boolean;
  disabled?: boolean;
  lockNote?: string; // tech prereq not met — shown in tooltip, button dimmed
  cls?: string;
  onClick: () => void;
}

const RESOURCE_FIELD = {
  [ResourceType.Wood]: 'wood',
  [ResourceType.Stone]: 'stone',
  [ResourceType.Food]: 'food',
  [ResourceType.Gold]: 'gold',
} as const satisfies Record<ResourceType, keyof ResourceCost>;

// Render a cost as "12🪵 5🪨 …" over whichever resources the cost names.
function costParts(cost: ResourceCost): { type: ResourceType; amount: number }[] {
  return ([ResourceType.Wood, ResourceType.Stone, ResourceType.Food, ResourceType.Gold] as const)
    .map((type) => ({ type, amount: cost[RESOURCE_FIELD[type]] ?? 0 }))
    .filter((p) => p.amount > 0);
}

function Tool({ icon, label, cost, active, disabled, lockNote, cls, onClick }: ToolProps) {
  const parts = cost ? costParts(cost) : [];
  const base = parts.length
    ? `${label} — ${parts.map((p) => `${p.amount} ${RESOURCE_DEFS[p.type].label}`).join(', ')}`
    : label;
  const title = lockNote ? `${label} — 🔒 ${lockNote}` : base;
  return (
    <button
      type="button"
      title={title}
      disabled={disabled || !!lockNote}
      onClick={onClick}
      className={`${styles.tool} ${active ? styles.toolActive : ''} ${cls ?? ''}`}
    >
      <span className={styles.toolIcon}>{icon}</span>
      <span className={styles.toolLabel}>{label}</span>
      {parts.length > 0 && (
        <span className={styles.toolCost}>
          {parts.map((p) => `${p.amount}${RESOURCE_DEFS[p.type].icon}`).join(' ')}
        </span>
      )}
    </button>
  );
}

export function BuildBar({
  wood,
  stone,
  food,
  gold,
  onTrain,
  onDemolish,
  onGatherAll,
  onTrade,
}: BuildBarProps) {
  const selB = useGameStore((s) => s.selectedBuilding);
  const buildMode = useGameStore((s) => s.buildMode);
  const setBuildMode = useGameStore((s) => s.setBuildMode);
  const demolishMode = useGameStore((s) => s.demolishMode);
  const setDemolishMode = useGameStore((s) => s.setDemolishMode);
  const ownedBuildings = useGameStore((s) => s.ownedBuildings);
  const [tab, setTab] = useState(0);

  // Tech-tree gate, mirrored from the module: a building/unit with a `requires`
  // is locked until the player owns that prerequisite. Shared hasPrereq keeps the
  // dim rule identical to the authoritative one.
  const owned = new Set(ownedBuildings) as Set<BuildingKind>;
  const lockNote = (def: { requires?: BuildingKind }): string | undefined =>
    hasPrereq(owned, def)
      ? undefined
      : `Requires ${BUILDING_DEFS[def.requires as 0].label}`;

  // The player's full stockpile drives every affordability check via the shared
  // canAfford contract — multi-resource costs (e.g. Tower's wood + stone) dim
  // correctly when any single resource is short.
  const stock = { wood, stone, food, gold };

  // The "Orders" group is global (always available). Market sells raw resources
  // for gold via the shared MARKET_RATE; disabled when there is nothing to sell.
  const orders = (
    <div className={styles.group}>
      <div className={styles.groupLabel}>Orders</div>
      <div className={styles.tools}>
        <Tool icon="🪓" label="Gather" cls={styles.green} onClick={onGatherAll} />
        <Tool
          icon="🪙"
          label="Sell Wood"
          disabled={wood < MARKET_LOT}
          onClick={() => onTrade(ResourceType.Wood, MARKET_LOT)}
        />
        <Tool
          icon="🪙"
          label="Sell Stone"
          disabled={stone < MARKET_LOT}
          onClick={() => onTrade(ResourceType.Stone, MARKET_LOT)}
        />
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
                  icon={u.icon}
                  label={u.label}
                  cost={u.cost}
                  disabled={!canAfford(stock, u.cost)}
                  lockNote={lockNote(u)}
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
                icon={d.icon}
                label={d.label}
                cost={d.cost}
                active={active}
                disabled={!active && !canAfford(stock, d.cost)}
                lockNote={lockNote(d)}
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
