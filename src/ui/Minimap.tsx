import { useEffect, useRef } from 'react';
import { useTable } from 'spacetimedb/react';
import { WORLD_SIZE, mapPresetById } from '../../shared/index.ts';
import { tables } from '../module_bindings';
import styles from './Minimap.module.css';

interface MinimapProps {
  onCanvas: (c: HTMLCanvasElement | null) => void;
  onClickWorld: (x: number, y: number) => void;
}

export function Minimap({ onCanvas, onClickWorld }: MinimapProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [configs] = useTable(tables.config);
  const cfg = configs[0];

  useEffect(() => {
    onCanvas(ref.current);
    return () => onCanvas(null);
  }, [onCanvas]);

  const handleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * WORLD_SIZE;
    const y = ((e.clientY - r.top) / r.height) * WORLD_SIZE;
    onClickWorld(x, y);
  };

  return (
    <div className={styles.wrap}>
      <canvas
        ref={ref}
        width={170}
        height={170}
        className={styles.canvas}
        onMouseDown={handleClick}
      />
      {cfg && (
        <span
          className={styles.mapLabel}
          title={`Map seed ${cfg.seed} — share to replay this map`}
        >
          {mapPresetById(cfg.preset).label} · #{cfg.seed}
        </span>
      )}
    </div>
  );
}
