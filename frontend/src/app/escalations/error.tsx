"use client";

import { useEffect } from "react";

export default function EscalationsError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error("Escalations page error:", error);
  }, [error]);

  return (
    <div className="flex flex-col items-center justify-center min-h-[60vh] space-y-4">
      <div className="text-center space-y-2">
        <h2 className="text-xl font-semibold">Failed to load escalations</h2>
        <p className="text-sm text-muted-foreground max-w-md">
          The escalation queue could not be loaded. Check that the dashboard
          API is running and the database is connected.
        </p>
      </div>
      <button
        onClick={reset}
        className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        Try again
      </button>
    </div>
  );
}
