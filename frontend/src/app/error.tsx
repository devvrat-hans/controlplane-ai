"use client";

import { useEffect } from "react";
import { BrandMark } from "@/components/layout/brand-mark";
import { BRAND } from "@/lib/brand";

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error(`${BRAND.full} dashboard error:`, error);
  }, [error]);

  return (
    <div className="flex flex-col items-center justify-center min-h-[60vh] space-y-4">
      <BrandMark size="lg" />
      <p className="text-[10px] font-mono uppercase tracking-[0.2em] text-muted-foreground">
        {BRAND.full}
      </p>
      <div className="text-center space-y-2">
        <h2 className="text-xl font-semibold">Something went wrong</h2>
        <p className="text-sm text-muted-foreground max-w-md">
          The {BRAND.productFull} dashboard encountered an error. This might be
          because the backend API is not running.
        </p>
        {error.digest && (
          <p className="text-xs font-mono text-muted-foreground/50">
            Error ID: {error.digest}
          </p>
        )}
      </div>
      <button
        onClick={reset}
        className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        Try again
      </button>
      <p className="text-[10px] font-mono text-muted-foreground/50">
        {BRAND.copyright} · {BRAND.version}
      </p>
    </div>
  );
}
