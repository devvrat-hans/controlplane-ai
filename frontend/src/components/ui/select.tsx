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
}

export function Select({
  value,
  onValueChange,
  options,
  placeholder,
  className,
  size = "sm",
}: SelectProps) {
  const sizeClasses = {
    sm: "h-8 text-xs px-2.5 pr-7",
    md: "h-9 text-sm px-3 pr-8",
  };

  return (
    <div className={cn("relative inline-flex", className)}>
      <select
        value={value}
        onChange={(e) => onValueChange(e.target.value)}
        className={cn(
          "appearance-none rounded-md border border-border bg-background font-medium transition-colors",
          "focus:outline-none focus:ring-2 focus:ring-ring/20 focus:border-ring/50",
          "hover:bg-accent/50 cursor-pointer",
          sizeClasses[size]
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
      {/* Custom chevron icon */}
      <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2">
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
