import { useEffect, useRef } from 'react';
import { WORLD_SIZE } from '../../shared/index.ts';
import styles from './Minimap.module.css';

interface MinimapProps {
  onCanvas: (c: HTMLCanvasElement | null) => void;
  onClickWorld: (x: number, y: number) => void;
}

export function Minimap({ onCanvas, onClickWorld }: MinimapProps) {
  const ref = useRef<HTMLCanvasElement>(null);

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
    </div>
  );
}
