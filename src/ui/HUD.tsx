import { ResourceBar } from "./ResourceBar";
import { BuildBar } from "./BuildBar";
import { CommandCard } from "./CommandCard";
import { Minimap } from "./Minimap";
import { Toasts } from "./Toasts";
import type { ResearchRowState } from "../../shared/index.ts";
import type { CompletedTech } from "../session/useResearch";
import styles from "./HUD.module.css";

export interface HUDProps {
  connected: boolean;
  ready: boolean;
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
  researchRows: ResearchRowState[];
  completedTechs: CompletedTech[];
  techMask: bigint;
  onTrain: (buildingId: string, kind: number) => void;
  onDemolish: (id: string) => void;
  onGatherAll: () => void;
  onTrade: (resType: number, amount: number) => void;
  onUngarrison: (buildingId: string) => void;
  onResearch: (buildingId: string, tech: number) => void;
  onAddAi: () => void;
  onSaveAndQuit: () => void;
  onLeave: () => void;
  onSetStance: (stance: number) => void;
  onMinimapCanvas: (c: HTMLCanvasElement | null) => void;
  onMinimapClick: (x: number, y: number) => void;
}

export function HUD(props: HUDProps) {
  return (
    <div className={styles.hud}>
      <ResourceBar
        name={props.name}
        faction={props.faction}
        wood={props.wood}
        stone={props.stone}
        food={props.food}
        gold={props.gold}
        starving={props.starving}
        peasants={props.peasants}
        soldiers={props.soldiers}
        pop={props.pop}
        cap={props.cap}
      />
      <Toasts />

      {props.ready && (
        <div className={styles.topRight}>
          <button className={styles.addAi} onClick={props.onAddAi}>
            ⚔ Add AI Opponent
          </button>
          <button className={styles.leave} onClick={props.onSaveAndQuit}>
            💾 Save &amp; Quit
          </button>
          <button className={styles.leave} onClick={props.onLeave}>
            ⮌ Leave Match
          </button>
        </div>
      )}

      <div className={styles.bottomBar}>
        <div className={styles.barLeft}>
          <CommandCard
            techMask={props.techMask}
            onSetStance={props.onSetStance}
          />
        </div>
        <div className={styles.barCenter}>
          <BuildBar
            wood={props.wood}
            stone={props.stone}
            food={props.food}
            gold={props.gold}
            researchRows={props.researchRows}
            completedTechs={props.completedTechs}
            onTrain={props.onTrain}
            onDemolish={props.onDemolish}
            onGatherAll={props.onGatherAll}
            onTrade={props.onTrade}
            onUngarrison={props.onUngarrison}
            onResearch={props.onResearch}
          />
        </div>
        <div className={styles.barRight}>
          <Minimap
            onCanvas={props.onMinimapCanvas}
            onClickWorld={props.onMinimapClick}
          />
        </div>
      </div>

      {props.connected && !props.ready && (
        <div className={styles.status}>Loading the Levant…</div>
      )}
    </div>
  );
}
