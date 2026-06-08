import {
  RESOURCE_DEFS,
  ResourceType,
  type ResearchRowState,
  type ResourceCost,
} from "../../shared/index.ts";
import styles from "./ResearchPanel.module.css";

interface ResearchPanelProps {
  buildingId: string;
  rows: ResearchRowState[];
  onResearch: (buildingId: string, tech: number) => void;
}

const RESOURCE_FIELD = {
  [ResourceType.Wood]: "wood",
  [ResourceType.Stone]: "stone",
  [ResourceType.Food]: "food",
  [ResourceType.Gold]: "gold",
} as const satisfies Record<ResourceType, keyof ResourceCost>;

function costParts(
  cost: ResourceCost,
): { type: ResourceType; amount: number }[] {
  return (
    [
      ResourceType.Wood,
      ResourceType.Stone,
      ResourceType.Food,
      ResourceType.Gold,
    ] as const
  )
    .map((type) => ({ type, amount: cost[RESOURCE_FIELD[type]] ?? 0 }))
    .filter((p) => p.amount > 0);
}

const STATUS_TIP: Record<ResearchRowState["status"], string> = {
  done: "Researched",
  in_progress: "Researching…",
  locked: "Locked",
  unaffordable: "Not enough resources",
  available: "Click to research",
};

export function ResearchPanel({
  buildingId,
  rows,
  onResearch,
}: ResearchPanelProps) {
  return (
    <div className={styles.group}>
      <div className={styles.groupLabel}>⚒️ Research</div>
      <div className={styles.tools}>
        {rows.map((r) => {
          const parts = costParts(r.cost);
          const clickable = r.status === "available";
          const title = r.lockNote
            ? `${r.label} — 🔒 ${r.lockNote}`
            : `${r.label} — ${STATUS_TIP[r.status]}${
                parts.length
                  ? ` (${parts
                      .map((p) => `${p.amount} ${RESOURCE_DEFS[p.type].label}`)
                      .join(", ")})`
                  : ""
              }`;
          return (
            <button
              key={r.tech}
              type="button"
              title={title}
              disabled={!clickable}
              onClick={() => onResearch(buildingId, r.tech)}
              className={`${styles.tool} ${styles[r.status]}`}
            >
              <span className={styles.toolIcon}>{r.icon}</span>
              <span className={styles.toolLabel}>{r.label}</span>

              {r.status === "done" && (
                <span className={styles.check}>✓ Done</span>
              )}

              {r.status === "in_progress" && (
                <span className={styles.progressTrack}>
                  <span
                    className={styles.progressFill}
                    style={{ width: `${Math.round(r.progress * 100)}%` }}
                  />
                </span>
              )}

              {r.status !== "done" && r.status !== "in_progress" && (
                <span className={styles.toolCost}>
                  {r.lockNote
                    ? "🔒"
                    : parts
                        .map((p) => `${p.amount}${RESOURCE_DEFS[p.type].icon}`)
                        .join(" ")}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
