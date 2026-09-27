import { BRAND } from "@/lib/brand";

/**
 * Route-level loading fallback (shown while a dashboard segment streams in).
 * Branded so a slow first paint never looks like a blank, unowned page.
 */
export default function Loading() {
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-3">
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-foreground border-t-transparent" />
      <p className="text-[11px] font-mono uppercase tracking-[0.18em] text-muted-foreground">
        {BRAND.full}
      </p>
      <p className="text-xs text-muted-foreground/60">Loading {BRAND.short.toLowerCase()}…</p>
    </div>
  );
}
