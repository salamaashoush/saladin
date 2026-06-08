import type { ReactNode } from 'react';
import styles from './Button.module.css';

interface ButtonProps {
  onClick?: () => void;
  disabled?: boolean;
  variant?: 'gold' | 'ghost' | 'green' | 'red';
  block?: boolean;
  title?: string;
  children: ReactNode;
}

export function Button({
  onClick,
  disabled,
  variant = 'gold',
  block,
  title,
  children,
}: ButtonProps) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={`${styles.btn} ${styles[variant]} ${block ? styles.block : ''}`}
    >
      {children}
    </button>
  );
}
