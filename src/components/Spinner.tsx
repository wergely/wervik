import styles from "./Spinner.module.css";

interface SpinnerProps {
  className?: string;
}

export function Spinner({ className }: SpinnerProps) {
  return <span className={`${styles.spinner} ${className ?? ""}`} role="status" aria-label="Loading" />;
}
