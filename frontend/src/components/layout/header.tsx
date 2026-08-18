"use client";

export function Header() {
  return (
    <header className="flex h-14 items-center justify-between border-b border-border px-6">
      <div className="flex items-center gap-4">
        <h1 className="text-sm font-medium text-foreground">Dashboard</h1>
      </div>
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2 rounded-md border border-border px-3 py-1.5">
          <div className="h-2 w-2 rounded-full bg-green-500 animate-pulse" />
          <span className="text-xs text-muted-foreground">Proxy active</span>
        </div>
        <div className="h-8 w-8 rounded-full bg-muted flex items-center justify-center">
          <span className="text-xs font-medium">A</span>
        </div>
      </div>
    </header>
  );
}
