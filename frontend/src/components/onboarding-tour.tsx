"use client";

import { useState, useEffect } from "react";

const TOUR_KEY = "cp-tour-completed";

const STEPS = [
  {
    title: "Welcome to ControlPlane",
    body: "Your AI governance layer — intercepting, scoring, and controlling every call to upstream AI providers.",
    icon: "🛡️",
  },
  {
    title: "Live Governance",
    body: "Watch verdicts flow in real-time. The fast-path runs in <10ms; the shadow-path does deep analysis asynchronously.",
    icon: "⚡",
  },
  {
    title: "Human-in-the-Loop",
    body: "Escalated cases get human review. Your decisions (confirm/override/dismiss) create precedents that teach the system via RAG retrieval.",
    icon: "🧑‍⚖️",
  },
  {
    title: "Keyboard Shortcuts",
    body: "Press ? anytime to see keyboard shortcuts. Navigate with 1–8 to jump between pages.",
    icon: "⌨️",
  },
];

export function OnboardingTour() {
  const [visible, setVisible] = useState(false);
  const [step, setStep] = useState(0);

  useEffect(() => {
    if (!sessionStorage.getItem(TOUR_KEY) && !localStorage.getItem(TOUR_KEY)) {
      setVisible(true);
    }
  }, []);

  const dismiss = () => {
    setVisible(false);
    localStorage.setItem(TOUR_KEY, "1");
  };

  const next = () => {
    if (step < STEPS.length - 1) {
      setStep((s) => s + 1);
    } else {
      dismiss();
    }
  };

  if (!visible) return null;

  const current = STEPS[step];

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="w-[380px] rounded-2xl border border-border bg-card p-8 shadow-2xl text-center">
        <div className="text-5xl mb-4">{current.icon}</div>
        <h3 className="text-lg font-semibold mb-2">{current.title}</h3>
        <p className="text-sm text-muted-foreground mb-6 leading-relaxed">{current.body}</p>

        {/* Step indicator */}
        <div className="flex items-center justify-center gap-1.5 mb-6">
          {STEPS.map((_, i) => (
            <div
              key={i}
              className={`h-1.5 rounded-full transition-all ${
                i === step ? "w-6 bg-primary" : i < step ? "w-1.5 bg-primary/40" : "w-1.5 bg-muted"
              }`}
            />
          ))}
        </div>

        <div className="flex gap-3">
          <button
            onClick={dismiss}
            className="flex-1 rounded-md border border-border px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
          >
            Skip tour
          </button>
          <button
            onClick={next}
            className="flex-1 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            {step < STEPS.length - 1 ? "Next" : "Get started"}
          </button>
        </div>

        <p className="text-[10px] text-muted-foreground/50 mt-4">
          {step + 1} of {STEPS.length}
        </p>
      </div>
    </div>
  );
}
