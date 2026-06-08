import type { ReactNode, CSSProperties } from 'react';
import styles from './Panel.module.css';

interface PanelProps {
  title?: string;
  padded?: boolean;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}

export function Panel({
  title,
  padded = true,
  className = '',
  style,
  children,
}: PanelProps) {
  return (
    <div
      className={`${styles.panel} ${padded ? styles.padded : ''} ${className}`}
      style={style}
    >
      {title && <div className={styles.title}>{title}</div>}
      {children}
    </div>
  );
}
