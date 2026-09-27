import { BRAND } from "@/lib/brand";
import { cn } from "@/lib/utils";

/**
 * The RTXCore monogram — the same three letters as `public/favicon.svg`, defined
 * once so the sidebar, login card, loading states and error states stay in sync.
 */
export function BrandMark({
  size = "sm",
  className,
}: {
  size?: "sm" | "lg";
  className?: string;
}) {
  return (
    <div
      aria-label={BRAND.vendor}
      role="img"
      className={cn(
        "flex shrink-0 items-center justify-center bg-primary",
        size === "sm" ? "h-6 w-6 rounded-md" : "h-12 w-12 rounded-lg",
        className
      )}
    >
      <span
        className={cn(
          "font-bold tracking-tight text-primary-foreground",
          size === "sm" ? "text-[9px]" : "text-lg"
        )}
      >
        {BRAND.mark}
      </span>
    </div>
  );
}

/**
 * Compact vendor lockup for surfaces with no room for the full wordmark (the
 * mobile header, where the sidebar is hidden).
 */
export function BrandLockup({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center gap-2", className)}>
      <BrandMark />
      <span className="text-[13px] font-semibold leading-none tracking-tight-brand">
        {BRAND.vendor}
      </span>
    </div>
  );
}
