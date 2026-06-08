import { ResourceBar } from './ResourceBar';
import { BuildBar } from './BuildBar';
import { CommandCard } from './CommandCard';
import { Minimap } from './Minimap';
import { Toasts } from './Toasts';
import styles from './HUD.module.css';

export interface HUDProps {
  connected: boolean;
  ready: boolean;
  name: string;
  faction: string;
  wood: number;
  peasants: number;
  soldiers: number;
  pop: number;
  cap: number;
  onTrain: (buildingId: string, kind: number) => void;
  onDemolish: (id: string) => void;
  onGatherAll: () => void;
  onAddAi: () => void;
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
        peasants={props.peasants}
        soldiers={props.soldiers}
        pop={props.pop}
        cap={props.cap}
      />
      <Toasts />

      {props.ready && (
        <button className={styles.addAi} onClick={props.onAddAi}>
          ⚔ Add AI Opponent
        </button>
      )}

      <div className={styles.bottomBar}>
        <div className={styles.barLeft}>
          <CommandCard />
        </div>
        <div className={styles.barCenter}>
          <BuildBar
            wood={props.wood}
            onTrain={props.onTrain}
            onDemolish={props.onDemolish}
            onGatherAll={props.onGatherAll}
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
