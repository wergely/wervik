import { useEffect, useRef } from "react";
import styles from "./OverflowMenu.module.css";

interface OverflowMenuProps {
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  onShowFiles?: () => void;
  onOpenSteamPage?: () => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
}

export function OverflowMenu({
  open,
  onOpen,
  onClose,
  onShowFiles,
  onOpenSteamPage,
  registerBackHandler,
  registerNavScope,
}: OverflowMenuProps) {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;

    registerNavScope(menuRef.current);
    registerBackHandler(() => () => onCloseRef.current());

    const firstItem = menuRef.current?.querySelector<HTMLElement>("[data-nav-item]");
    firstItem?.focus();

    function handlePointerDown(e: PointerEvent) {
      if (!wrapperRef.current?.contains(e.target as Node)) {
        onCloseRef.current();
      }
    }
    window.addEventListener("pointerdown", handlePointerDown);

    return () => {
      registerNavScope(null);
      registerBackHandler(null);
      window.removeEventListener("pointerdown", handlePointerDown);
      // Only reached when closing (this effect's body only ran while open),
      // never on the initial mount — safe to reclaim focus here.
      triggerRef.current?.focus();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    <div className={styles.wrapper} ref={wrapperRef}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.trigger}
        data-nav-item
        onClick={() => (open ? onClose() : onOpen())}
        aria-label="More actions"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        ⋯
      </button>
      {open && (
        <div className={styles.menu} ref={menuRef} role="menu">
          {onShowFiles && (
            <button
              type="button"
              className={styles.item}
              data-nav-item
              role="menuitem"
              onClick={() => {
                onShowFiles();
                onClose();
              }}
            >
              📁 Show game files
            </button>
          )}
          {onOpenSteamPage && (
            <button
              type="button"
              className={styles.item}
              data-nav-item
              role="menuitem"
              onClick={() => {
                onOpenSteamPage();
                onClose();
              }}
            >
              🔗 Open Steam page
            </button>
          )}
        </div>
      )}
    </div>
  );
}
