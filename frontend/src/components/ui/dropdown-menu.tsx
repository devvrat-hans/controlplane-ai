"use client";

import {
  useState,
  useRef,
  useEffect,
  createContext,
  useContext,
  useCallback,
  type ReactNode,
  type RefObject,
} from "react";
import { cn } from "@/lib/utils";

// ─── Context ─────────────────────────────────────────────────────────────
interface DropdownContextValue {
  open: boolean;
  setOpen: (open: boolean) => void;
  triggerRef: RefObject<HTMLDivElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
}

const DropdownContext = createContext<DropdownContextValue | null>(null);

function useDropdown() {
  const ctx = useContext(DropdownContext);
  if (!ctx) throw new Error("Dropdown compound components must be used inside <Dropdown>");
  return ctx;
}

// ─── Root ────────────────────────────────────────────────────────────────
export function Dropdown({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  // Close on outside click — check both trigger AND content
  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      const target = e.target as Node;
      const clickedInsideTrigger = triggerRef.current?.contains(target);
      const clickedInsideContent = contentRef.current?.contains(target);
      if (!clickedInsideTrigger && !clickedInsideContent) {
        setOpen(false);
      }
    }
    function handleEscape(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    // Use mousedown so it fires before click
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [open]);

  return (
    <DropdownContext.Provider value={{ open, setOpen, triggerRef, contentRef }}>
      <div className={cn("relative", className)}>{children}</div>
    </DropdownContext.Provider>
  );
}

// ─── Trigger ─────────────────────────────────────────────────────────────
export function DropdownTrigger({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  const { open, setOpen, triggerRef } = useDropdown();

  return (
    <div
      ref={triggerRef}
      role="button"
      tabIndex={0}
      onClick={() => setOpen(!open)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setOpen(!open);
        }
      }}
      className={cn(
        "flex items-center gap-2 rounded-lg px-2 py-1.5 transition-colors cursor-pointer select-none",
        "hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        className
      )}
    >
      {children}
    </div>
  );
}

// ─── Content (positioned panel) ──────────────────────────────────────────
export function DropdownContent({
  children,
  align = "end",
  className,
  width = "w-56",
}: {
  children: ReactNode;
  align?: "start" | "end";
  className?: string;
  width?: string;
}) {
  const { open, triggerRef, contentRef } = useDropdown();
  const panelRef = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState({ top: 0, right: 0, left: 0 });
  const [positioned, setPositioned] = useState(false);

  // Sync panelRef -> contentRef so outside-click handler works
  useEffect(() => {
    (contentRef as React.MutableRefObject<HTMLDivElement | null>).current = panelRef.current;
  });

  const calculatePosition = useCallback(() => {
    if (!triggerRef.current) return;
    const triggerRect = triggerRef.current.getBoundingClientRect();
    const gap = 8;

    const top = triggerRect.bottom + gap;
    let right = 0;
    let left = 0;

    if (align === "end") {
      right = window.innerWidth - triggerRect.right;
      const estWidth = 224;
      const contentLeft = window.innerWidth - right - estWidth;
      if (contentLeft < 8) {
        right = window.innerWidth - estWidth - 8;
      }
    } else {
      left = triggerRect.left;
      const estWidth = 224;
      if (left + estWidth > window.innerWidth - 8) {
        left = window.innerWidth - estWidth - 8;
      }
    }

    setCoords({ top, right, left });
    setPositioned(true);
  }, [align, triggerRef]);

  useEffect(() => {
    if (open) {
      requestAnimationFrame(calculatePosition);
    } else {
      setPositioned(false);
    }
  }, [open, calculatePosition]);

  if (!open) return null;

  return (
    <div
      ref={panelRef}
      className={cn(
        "fixed z-[9999] rounded-xl border border-border bg-popover shadow-lg py-1.5",
        width,
        className
      )}
      style={{
        top: `${coords.top}px`,
        ...(align === "end"
          ? { right: `${coords.right}px` }
          : { left: `${coords.left}px` }),
        opacity: positioned ? 1 : 0,
        pointerEvents: positioned ? "auto" : "none",
      }}
    >
      {children}
    </div>
  );
}

// ─── Item ────────────────────────────────────────────────────────────────
export function DropdownItem({
  children,
  onClick,
  href,
  icon,
  danger,
  className,
}: {
  children: ReactNode;
  onClick?: () => void;
  href?: string;
  icon?: ReactNode;
  danger?: boolean;
  className?: string;
}) {
  const { setOpen } = useDropdown();

  const handleClick = (e: React.MouseEvent) => {
    // Call the onClick handler first
    onClick?.();

    // Close dropdown
    setOpen(false);

    // Navigate after a frame to ensure unmount completes
    if (href) {
      e.preventDefault();
      requestAnimationFrame(() => {
        window.location.href = href;
      });
    }
  };

  const classes = cn(
    "flex items-center gap-2.5 px-3 py-2 text-[13px] w-full transition-colors cursor-pointer",
    "hover:bg-accent focus-visible:outline-none focus-visible:bg-accent",
    danger
      ? "text-destructive hover:bg-destructive/5"
      : "text-foreground",
    className
  );

  if (href) {
    return (
      <a
        href={href}
        onClick={(e) => {
          e.preventDefault();
          handleClick(e);
        }}
        className={classes}
      >
        {icon && <span className="w-4 h-4 shrink-0 opacity-70">{icon}</span>}
        {children}
      </a>
    );
  }

  return (
    <button onClick={handleClick} className={classes}>
      {icon && <span className="w-4 h-4 shrink-0 opacity-70">{icon}</span>}
      {children}
    </button>
  );
}

// ─── Separator ───────────────────────────────────────────────────────────
export function DropdownSeparator() {
  return <div className="border-t border-border my-1.5" />;
}

// ─── Label ───────────────────────────────────────────────────────────────
export function DropdownLabel({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("px-3.5 py-2.5 border-b border-border", className)}>
      {children}
    </div>
  );
}
