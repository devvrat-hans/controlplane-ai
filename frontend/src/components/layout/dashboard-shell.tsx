"use client";

import { useEffect, useState } from "react";
import { useRouter, usePathname } from "next/navigation";
import { Sidebar, MobileSidebar } from "./sidebar";
import { Header } from "./header";
import { BrandMark } from "./brand-mark";
import { OnboardingTour } from "@/components/onboarding-tour";
import { BRAND, brandTitle, pageTitleFor } from "@/lib/brand";

const SHORTCUTS = [
  { key: "1", label: "Overview", href: "/" },
  { key: "2", label: "Live Stream", href: "/stream" },
  { key: "3", label: "Requests", href: "/requests" },
  { key: "4", label: "Analytics", href: "/analytics" },
  { key: "5", label: "Policies", href: "/policies" },
  { key: "6", label: "Escalations", href: "/escalations" },
  { key: "7", label: "Cost", href: "/cost" },
  { key: "8", label: "Audit", href: "/audit" },
  { key: "9", label: "Settings", href: "/settings" },
];

export function DashboardShell({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);

  useEffect(() => {
    const token = sessionStorage.getItem("cp-token");
    if (!token) {
      router.replace("/login");
    } else {
      setAuthenticated(true);
    }
  }, [router]);

  // Brand every browser tab: "Audit · RTXCore ControlPlane AI". Done once here
  // rather than per page, because the pages are client components and cannot
  // export their own <Metadata>.
  useEffect(() => {
    document.title = brandTitle(pageTitleFor(pathname));
  }, [pathname]);

  // Keyboard shortcuts for quick navigation
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName;
      const isInput = tag === "INPUT" || tag === "TEXTAREA" || (e.target as HTMLElement).isContentEditable;

      if (e.key === "?" && !isInput) {
        e.preventDefault();
        setShowShortcuts((s) => !s);
        return;
      }
      if (e.key === "Escape" && showShortcuts) {
        setShowShortcuts(false);
        return;
      }
      if (isInput) return;

      const map = Object.fromEntries(SHORTCUTS.map((s) => [s.key, s.href]));
      if (map[e.key]) {
        e.preventDefault();
        router.push(map[e.key]);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [router, showShortcuts]);

  if (!authenticated) {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-4 bg-background">
        <BrandMark size="lg" />
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-foreground border-t-transparent" />
        <div className="text-center">
          <p className="text-[11px] font-mono uppercase tracking-[0.18em] text-muted-foreground">
            {BRAND.full}
          </p>
          <p className="mt-1 text-[11px] text-muted-foreground/60">
            Loading {BRAND.short.toLowerCase()}…
          </p>
        </div>
      </div>
    );
  }

  return (
    <>
      <OnboardingTour />
      <div className="flex h-screen overflow-hidden bg-background">
        <Sidebar />
        <MobileSidebar open={mobileOpen} onClose={() => setMobileOpen(false)} />
        <div className="flex flex-1 flex-col overflow-hidden">
          <Header />
          <main className="flex-1 overflow-y-auto p-5 md:p-8">{children}</main>

          {/* Brand footer — present on every dashboard page, including exports
              and demo screenshots that crop out the sidebar. */}
          <footer className="flex h-9 shrink-0 items-center justify-between gap-3 border-t border-border bg-card px-5">
            <div className="flex min-w-0 items-center gap-2">
              <span className="text-[10px] font-semibold tracking-tight-brand text-muted-foreground">
                {BRAND.full}
              </span>
              <span className="hidden truncate text-[10px] text-muted-foreground/50 sm:inline">
                {BRAND.tagline}
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-3 text-[10px] text-muted-foreground/50">
              <span className="hidden font-mono uppercase tracking-[0.12em] md:inline">
                Local deployment
              </span>
              <span className="font-mono">{BRAND.version}</span>
            </div>
          </footer>
        </div>
      </div>

      {/* Keyboard shortcuts modal */}
      {showShortcuts && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-[2px]" onClick={() => setShowShortcuts(false)}>
          <div className="w-[320px] rounded-xl border border-border bg-card p-6 shadow-xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-sm font-semibold">Keyboard Shortcuts</h3>
              <button onClick={() => setShowShortcuts(false)} className="text-muted-foreground hover:text-foreground text-xs">Esc</button>
            </div>
            <div className="space-y-2">
              {SHORTCUTS.map((s) => (
                <div key={s.key} className="flex items-center justify-between">
                  <span className="text-sm text-muted-foreground">{s.label}</span>
                  <kbd className="px-2 py-0.5 rounded border border-border bg-muted text-[11px] font-mono font-medium">{s.key}</kbd>
                </div>
              ))}
            </div>
            <div className="mt-4 pt-3 border-t border-border">
              <div className="flex items-center justify-between">
                <span className="text-sm text-muted-foreground">Show this help</span>
                <kbd className="px-2 py-0.5 rounded border border-border bg-muted text-[11px] font-mono font-medium">?</kbd>
              </div>
            </div>
            <p className="mt-3 text-[9px] font-mono uppercase tracking-[0.14em] text-muted-foreground/40">
              {BRAND.full}
            </p>
          </div>
        </div>
      )}
    </>
  );
}
