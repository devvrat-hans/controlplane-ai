"use client";

import { cn } from "@/lib/utils";

interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps {
  value: string;
  onValueChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  className?: string;
  size?: "sm" | "md";
  /** Accessible name — use when there is no visible <label> (e.g. "Per page"). */
  ariaLabel?: string;
  /** Extra classes for the <select> itself — the place to set a width. */
  selectClassName?: string;
}

/**
 * The single dropdown used everywhere in the dashboard.
 *
 * The arrow is ours, not the browser's: `appearance-none` removes the native
 * control, and the chevron is anchored inside the wrapper with the same inset as
 * the select's right padding (`pr-8`/`pr-9`), so it can never straddle the right
 * border.
 *
 * Sizing: the wrapper is `w-fit`, so it is exactly as wide as the box inside it —
 * and `w-fit` also stops a grid or column-flex parent from stretching the wrapper
 * past the select (a stretched wrapper is the other way the chevron ends up off
 * the border). The select keeps its intrinsic width, which fills to the widest
 * option; do not put `w-full` on the select, since a percentage width makes
 * Chrome size it to the *selected* option instead, clipping longer choices.
 * Pass `className` for spacing and `selectClassName` to size the box itself.
 */
export function Select({
  value,
  onValueChange,
  options,
  placeholder,
  className,
  size = "sm",
  ariaLabel,
  selectClassName,
}: SelectProps) {
  const sizeClasses = {
    sm: "h-8 text-xs pl-2.5 pr-8",
    md: "h-9 text-sm pl-3 pr-9",
  };

  return (
    <div className={cn("relative inline-flex w-fit max-w-full", className)}>
      <select
        value={value}
        onChange={(e) => onValueChange(e.target.value)}
        aria-label={ariaLabel}
        className={cn(
          "min-w-0 appearance-none rounded-md border border-border bg-background font-medium transition-colors",
          "focus:outline-none focus:ring-2 focus:ring-ring/20 focus:border-ring/50",
          "hover:bg-accent/50 cursor-pointer",
          sizeClasses[size],
          selectClassName
        )}
      >
        {placeholder && (
          <option value="">{placeholder}</option>
        )}
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      {/* Custom chevron — 10px inset, 14px wide, inside the 32/36px right padding. */}
      <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2.5">
        <ChevronDownIcon className="h-3.5 w-3.5 text-muted-foreground" />
      </div>
    </div>
  );
}

function ChevronDownIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}
