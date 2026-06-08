import {
  Stance,
  UNIT_DEFS,
  effectiveUnitDef,
  type UnitKind,
} from "../../shared/index.ts";
import { useGameStore } from "../store/gameStore";
import styles from "./CommandCard.module.css";

const STANCES: Array<[number, string, string]> = [
  [Stance.Aggressive, "⚔️", "Aggressive"],
  [Stance.Defensive, "🛡️", "Defensive"],
  [Stance.HoldGround, "⛰️", "Hold"],
];

export function CommandCard({
  techMask,
  onSetStance,
}: {
  techMask: bigint;
  onSetStance: (stance: number) => void;
}) {
  const sel = useGameStore((s) => s.selection);
  if (sel.total === 0) return null;

  const hpColor =
    sel.avgHp > 0.5 ? "#5b8a3a" : sel.avgHp > 0.25 ? "#c9a227" : "#b6402f";
  const moraleColor =
    sel.avgMorale > 0.5
      ? "#3f73c4"
      : sel.avgMorale > 0.25
        ? "#c9a227"
        : "#b6402f";

  // One row per selected UnitKind, ordered by the roster enum, labelled and
  // iconed straight from UNIT_DEFS — new units appear with no edits here.
  const rows = Object.entries(sel.byKind)
    .map(([kind, count]) => ({ kind: Number(kind) as UnitKind, count }))
    .filter((r) => r.count > 0 && UNIT_DEFS[r.kind])
    .sort((a, b) => a.kind - b.kind);

  return (
    <div className={styles.card}>
      <div className={styles.title}>Selection</div>
      <div className={styles.total}>
        {sel.total} unit{sel.total > 1 ? "s" : ""}
      </div>
      <div className={styles.rows}>
        {rows.map(({ kind, count }) => {
          const base = UNIT_DEFS[kind];
          // Fold the owner's researched techs onto the base so the card shows the
          // upgraded attack/hp the unit actually fights with — same pure helper the
          // module uses, so the numbers match the authority exactly.
          const eff = effectiveUnitDef(kind, techMask);
          const upgraded =
            eff.attack !== base.attack ||
            eff.maxHp !== base.maxHp ||
            eff.armorClass !== base.armorClass;
          const tip = `⚔️ ${eff.attack}  ❤️ ${eff.maxHp}${
            upgraded ? "  (upgraded)" : ""
          }`;
          return (
            <div className={styles.row} key={kind} title={tip}>
              <span className={styles.k}>
                {base.icon} {base.label}
                {upgraded && <span className={styles.upgraded}>▲</span>}
              </span>
              <span className={styles.v}>{count}</span>
            </div>
          );
        })}
      </div>
      {sel.hasCombat && (
        <div className={styles.stances}>
          {STANCES.map(([s, icon, label]) => (
            <button
              key={s}
              type="button"
              className={styles.stance}
              title={label}
              onClick={() => onSetStance(s)}
            >
              {icon}
            </button>
          ))}
        </div>
      )}
      <div className={styles.hpbar}>
        <div
          className={styles.hpfill}
          style={{
            width: `${Math.round(sel.avgHp * 100)}%`,
            background: hpColor,
          }}
        />
      </div>
      {sel.hasCombat && (
        <>
          <div className={styles.barLabel}>
            <span>Morale</span>
            {sel.routingCount > 0 && (
              <span className={styles.routing}>
                🏳️ {sel.routingCount} routing
              </span>
            )}
          </div>
          <div className={styles.hpbar}>
            <div
              className={styles.hpfill}
              style={{
                width: `${Math.round(sel.avgMorale * 100)}%`,
                background: moraleColor,
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}
