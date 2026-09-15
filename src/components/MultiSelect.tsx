import { useEffect, useRef, useState } from "react";
import styles from "./MultiSelect.module.css";

export interface MultiSelectOption<T extends string> {
  value: T;
  label: string;
}

interface MultiSelectProps<T extends string> {
  allLabel: string;
  ariaLabel: string;
  options: readonly MultiSelectOption<T>[];
  selected: T[];
  selectedLabel: (count: number) => string;
  onChange: (values: T[]) => void;
}

export function MultiSelect<T extends string>({
  allLabel,
  ariaLabel,
  options,
  selected,
  selectedLabel,
  onChange,
}: MultiSelectProps<T>) {
  const [open, setOpen] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;

    function handlePointerDown(event: PointerEvent) {
      if (!wrapperRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    // Capture Escape before the app-level back handler can navigate away from
    // the Library. The first Escape closes this filter; the next one backs out.
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      triggerRef.current?.focus();
    }

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown, true);
    };
  }, [open]);

  const displayLabel =
    selected.length === 0
      ? allLabel
      : selected.length === 1
        ? options.find((option) => option.value === selected[0])?.label ?? selected[0]
        : selectedLabel(selected.length);

  function toggle(value: T) {
    const next = selected.includes(value)
      ? selected.filter((item) => item !== value)
      : [...selected, value];
    // Empty is the compact persisted representation for "all". Selecting
    // every option is therefore equivalent to clearing the filter.
    onChange(next.length === options.length ? [] : next);
  }

  return (
    <div className={styles.wrapper} ref={wrapperRef}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.trigger}
        data-nav-item
        onClick={() => setOpen((value) => !value)}
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        <span className={styles.triggerLabel}>{displayLabel}</span>
        <span className={styles.chevron} aria-hidden="true">⌄</span>
      </button>
      {open && (
        <div className={styles.menu} role="listbox" aria-label={ariaLabel} aria-multiselectable="true">
          <label className={`${styles.option} ${styles.allOption}`}>
            <input
              type="checkbox"
              checked={selected.length === 0}
              onChange={() => onChange([])}
              data-nav-item
            />
            <span>{allLabel}</span>
          </label>
          {options.map((option) => (
            <label className={styles.option} key={option.value}>
              <input
                type="checkbox"
                checked={selected.includes(option.value)}
                onChange={() => toggle(option.value)}
                data-nav-item
              />
              <span>{option.label}</span>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
