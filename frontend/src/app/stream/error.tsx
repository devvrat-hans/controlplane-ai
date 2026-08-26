"use client";

import { useEffect } from "react";

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error("Stream page error:", error);
  }, [error]);

  return (
    <div className="flex flex-col items-center justify-center min-h-[60vh] space-y-4">
      <div className="text-center space-y-2">
        <h2 className="text-xl font-semibold">Stream unavailable</h2>
        <p className="text-sm text-muted-foreground max-w-md">
          The live stream could not connect. Check that the dashboard API is running and SSE is enabled.
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
        Reconnect
      </button>
    </div>
  );
}
