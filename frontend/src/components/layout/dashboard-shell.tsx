"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Sidebar, MobileSidebar } from "./sidebar";
import { Header } from "./header";
import { OnboardingTour } from "@/components/onboarding-tour";

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
      <div className="flex h-screen items-center justify-center bg-background">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-foreground border-t-transparent" />
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
          </div>
        </div>
      )}
    </>
  );
}
