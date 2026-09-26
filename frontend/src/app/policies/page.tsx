"use client";

import { useState, useEffect } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import { useUser, canEditPolicies } from "@/lib/auth";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface PolicyThresholds {
  block_threshold: number;
  escalate_threshold: number;
  max_tokens_per_request: number;
  retry_max_count: number;
  // Guardrails sidecar checks
  pii_detection: boolean;
  toxicity_detection: boolean;
  bias_detection: boolean;
  // Fast-path checks
  unsafe_content_enabled: boolean;
  secret_detection_enabled: boolean;
  // Shadow-path checks
  prompt_injection_enabled: boolean;
  hallucination_detection_enabled: boolean;
  groundedness_enabled: boolean;
  verbosity_enabled: boolean;
  semantic_pii_enabled: boolean;
  // Decision-model judge (Laya / Jev). The process-level master switch is the
  // DECISION_JUDGE env var; this per-app flag can only opt an app OUT.
  decision_judge_enabled: boolean;
}

const DEFAULT_THRESHOLDS: PolicyThresholds = {
  block_threshold: 0.9,
  escalate_threshold: 0.6,
  max_tokens_per_request: 4000,
  retry_max_count: 3,
  pii_detection: true,
  toxicity_detection: true,
  bias_detection: true,
  unsafe_content_enabled: true,
  secret_detection_enabled: true,
  prompt_injection_enabled: true,
  hallucination_detection_enabled: true,
  groundedness_enabled: true,
  verbosity_enabled: true,
  semantic_pii_enabled: true,
  decision_judge_enabled: true,
};

interface AppInfo {
  id: string;
  name: string;
  data_governance_level?: string;
}

export default function PoliciesPage() {
  const user = useUser();
  const isEditable = user ? canEditPolicies(user.role) : true;
  const [apps, setApps] = useState<AppInfo[]>([]);
  const [selectedApp, setSelectedApp] = useState("");

  const [fetchError, setFetchError] = useState<string | null>(null);
  const [policyVersion, setPolicyVersion] = useState<number | null>(null);
  const [activeProfile, setActiveProfile] = useState<string | null>(null);

  useEffect(() => {
    fetch(`${API_BASE}/api/v1/apps`)
      .then((r) => r.json())
      .then((data: AppInfo[]) => {
        setApps(data);
        if (data.length > 0) setSelectedApp(data[0].id);
        setFetchError(null);
      })
      .catch((err) => {
        console.error("Failed to load apps:", err);
        setFetchError("Could not connect to API — is the backend running?");
      });
  }, []);

  const [thresholds, setThresholds] = useState<PolicyThresholds>(DEFAULT_THRESHOLDS);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  // Policy-wise effectiveness stats (blocks & escalations per check)
  interface PolicyCheckStat {
    check_name: string;
    axis: string;
    total: number;
    passes: number;
    edits: number;
    escalates: number;
    blocks: number;
    confirmed: number;
    overridden: number;
    dismissed: number;
    precision: number;
  }
  const [policyStats, setPolicyStats] = useState<PolicyCheckStat[]>([]);

  useEffect(() => {
    if (!selectedApp) return;
    const loadStats = () => {
      fetch(`${API_BASE}/api/v1/stats/policy?app_id=${selectedApp}&window_hours=168`)
        .then((r) => r.json())
        .then((data) => setPolicyStats(data.checks || []))
        .catch((err) => console.error("Failed to load policy stats:", err));
    };
    loadStats();
    const id = setInterval(loadStats, 15000);
    return () => clearInterval(id);
  }, [selectedApp]);

  // Regulatory profiles (R2.2)
  interface PolicyProfileInfo {
    id: string;
    name: string;
    description: string;
    geography: string;
    industry: string;
    risk_appetite: string;
    default_thresholds: Record<string, unknown>;
    regulations: string[];
  }
  const [profiles, setProfiles] = useState<PolicyProfileInfo[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<string | null>(null);

  useEffect(() => {
    fetch(`${API_BASE}/api/v1/profiles`)
      .then((r) => r.json())
      .then((data: { profiles: PolicyProfileInfo[] }) => setProfiles(data.profiles || []))
      .catch((err) => console.error("Failed to load profiles:", err));
  }, []);

  const selectProfile = (profile: PolicyProfileInfo) => {
    setSelectedProfile(profile.id);
    // Load this profile's stored thresholds into the form
    const t = profile.default_thresholds;
    const perf = (t.performance ?? {}) as Record<string, unknown>;
    const cost = (t.cost ?? {}) as Record<string, unknown>;
    const resp = (t.responsibility ?? {}) as Record<string, unknown>;
    setThresholds({
      block_threshold: (perf.block_threshold ?? resp.block_threshold ?? DEFAULT_THRESHOLDS.block_threshold) as number,
      escalate_threshold: (perf.escalate_threshold ?? resp.escalate_threshold ?? perf.groundedness_threshold ?? DEFAULT_THRESHOLDS.escalate_threshold) as number,
      max_tokens_per_request: (cost.max_tokens_per_request ?? DEFAULT_THRESHOLDS.max_tokens_per_request) as number,
      retry_max_count: (cost.retry_max ?? DEFAULT_THRESHOLDS.retry_max_count) as number,
      pii_detection: (resp.pii_detection ?? DEFAULT_THRESHOLDS.pii_detection) as boolean,
      toxicity_detection: (resp.toxicity_detection ?? DEFAULT_THRESHOLDS.toxicity_detection) as boolean,
      bias_detection: (resp.bias_detection ?? DEFAULT_THRESHOLDS.bias_detection) as boolean,
      unsafe_content_enabled: ((resp.unsafe_action ?? "block") !== "off") as boolean,
      secret_detection_enabled: ((resp.pii_action ?? "edit") !== "off") as boolean,
      prompt_injection_enabled: DEFAULT_THRESHOLDS.prompt_injection_enabled,
      hallucination_detection_enabled: ((perf.hallucination_action ?? "escalate") !== "off") as boolean,
      groundedness_enabled: DEFAULT_THRESHOLDS.groundedness_enabled,
      verbosity_enabled: DEFAULT_THRESHOLDS.verbosity_enabled,
      semantic_pii_enabled: DEFAULT_THRESHOLDS.semantic_pii_enabled,
      decision_judge_enabled: DEFAULT_THRESHOLDS.decision_judge_enabled,
    });
  };

  const deselectProfile = () => {
    setSelectedProfile(null);
    if (selectedApp) loadPolicy(selectedApp);
  };

  const loadPolicy = async (appId: string) => {
    try {
      const res = await fetch(`${API_BASE}/api/v1/policies/${appId}`);
      if (res.ok) {
        const data = await res.json();
        setPolicyVersion(data.version ?? null);
        const profileId = data.policies?.[0]?.profile ?? null;
        setActiveProfile(profileId);

        // If the app has an active profile, use the profile's authoritative
        // defaults so the UI always reflects the profile — even if the stored
        // threshold_config drifted (e.g. from a manual slider save).
        if (profileId) {
          const matchedProfile = profiles.find((p) => p.id === profileId);
          if (matchedProfile) {
            selectProfile(matchedProfile);
            return;
          }
          // Profile not loaded yet — try fetching it directly
          try {
            const profRes = await fetch(`${API_BASE}/api/v1/profiles`);
            if (profRes.ok) {
              const profData = await profRes.json();
              const freshProfiles: PolicyProfileInfo[] = profData.profiles || [];
              setProfiles(freshProfiles);
              const found = freshProfiles.find((p) => p.id === profileId);
              if (found) {
                selectProfile(found);
                return;
              }
            }
          } catch { /* fall through to stored config */ }
        }

        // No active profile — use the stored per-app policy config
        const config = data.merged ?? data.policies?.[0]?.config;
        const checks = config?.checks ?? {};
        if (config) {
          setThresholds({
            block_threshold: config.block_threshold ?? DEFAULT_THRESHOLDS.block_threshold,
            escalate_threshold: config.escalate_threshold ?? config.groundedness_threshold ?? DEFAULT_THRESHOLDS.escalate_threshold,
            max_tokens_per_request: config.max_tokens_per_request ?? DEFAULT_THRESHOLDS.max_tokens_per_request,
            retry_max_count: config.retry_max ?? config.retry_max_count ?? DEFAULT_THRESHOLDS.retry_max_count,
            pii_detection: checks.pii_detection ?? config.pii_detection ?? DEFAULT_THRESHOLDS.pii_detection,
            toxicity_detection: checks.toxicity_detection ?? config.toxicity_detection ?? DEFAULT_THRESHOLDS.toxicity_detection,
            bias_detection: checks.bias_detection ?? config.bias_detection ?? DEFAULT_THRESHOLDS.bias_detection,
            unsafe_content_enabled:
              checks.unsafe_content_enabled ?? (config.unsafe_content_enabled ?? (config.unsafe_action ? config.unsafe_action !== "off" : DEFAULT_THRESHOLDS.unsafe_content_enabled)),
            secret_detection_enabled:
              checks.secret_detection_enabled ?? (config.secret_detection_enabled ?? (config.pii_action ? config.pii_action !== "off" : DEFAULT_THRESHOLDS.secret_detection_enabled)),
            prompt_injection_enabled: checks.prompt_injection_enabled ?? config.prompt_injection_enabled ?? DEFAULT_THRESHOLDS.prompt_injection_enabled,
            hallucination_detection_enabled: checks.hallucination_detection_enabled ?? config.hallucination_detection_enabled ?? DEFAULT_THRESHOLDS.hallucination_detection_enabled,
            groundedness_enabled: checks.groundedness_enabled ?? config.groundedness_enabled ?? DEFAULT_THRESHOLDS.groundedness_enabled,
            verbosity_enabled: checks.verbosity_enabled ?? config.verbosity_enabled ?? DEFAULT_THRESHOLDS.verbosity_enabled,
            semantic_pii_enabled: checks.semantic_pii_enabled ?? config.semantic_pii_enabled ?? DEFAULT_THRESHOLDS.semantic_pii_enabled,
            decision_judge_enabled: checks.decision_judge_enabled ?? config.decision_judge_enabled ?? DEFAULT_THRESHOLDS.decision_judge_enabled,
          });
          return;
        }
      }
    } catch { /* fall through to defaults */ }
    setThresholds(DEFAULT_THRESHOLDS);
    setPolicyVersion(null);
  };

  const toggle = (key: keyof PolicyThresholds) => {
    if (!isEditable) return;
    setThresholds((t) => ({ ...t, [key]: !t[key] }));
  };

  // Load policy whenever selectedApp changes (including initial load)
  useEffect(() => {
    if (selectedApp) {
      setSelectedProfile(null);
      loadPolicy(selectedApp);
    }
  }, [selectedApp]);

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);

    try {
      if (selectedProfile) {
        // Save thresholds to the profile's default_thresholds
        const profilePayload = {
          performance: {
            groundedness_threshold: thresholds.escalate_threshold,
            hallucination_action: thresholds.hallucination_detection_enabled ? "escalate" : "off",
            block_threshold: thresholds.block_threshold,
            escalate_threshold: thresholds.escalate_threshold,
          },
          cost: {
            max_tokens_per_request: thresholds.max_tokens_per_request,
            retry_max: thresholds.retry_max_count,
          },
          responsibility: {
            bias_threshold: 0.7,
            pii_action: thresholds.secret_detection_enabled ? "edit" : "off",
            unsafe_action: thresholds.unsafe_content_enabled ? "block" : "off",
            block_threshold: thresholds.block_threshold,
            escalate_threshold: thresholds.escalate_threshold,
          },
        };
        const res = await fetch(`${API_BASE}/api/v1/profiles/${selectedProfile}`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(profilePayload),
        });
        if (res.ok) {
          // Re-apply the profile to the app so the policies table stays in
          // sync with the profile's updated defaults.
          if (selectedApp) {
            await fetch(`${API_BASE}/api/v1/policies/${selectedApp}/profile`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ profile_id: selectedProfile }),
            });
          }
          setSaved(true);
          setTimeout(() => setSaved(false), 3000);
          const profilesRes = await fetch(`${API_BASE}/api/v1/profiles`);
          if (profilesRes.ok) {
            const data = await profilesRes.json();
            setProfiles(data.profiles || []);
          }
        }
      } else {
        // Save thresholds to the app's active policies
        const res = await fetch(`${API_BASE}/api/v1/policies/${selectedApp}`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(thresholds),
        });
        if (res.ok) {
          setSaved(true);
          setTimeout(() => setSaved(false), 3000);
          loadPolicy(selectedApp);
        }
      }
    } catch {
      // API might not be running
    } finally {
      setSaving(false);
    }
  };

  const enabledCount = [
    "unsafe_content_enabled",
    "secret_detection_enabled",
    "prompt_injection_enabled",
    "hallucination_detection_enabled",
    "groundedness_enabled",
    "verbosity_enabled",
    "semantic_pii_enabled",
    "pii_detection",
    "toxicity_detection",
    "bias_detection",
    "decision_judge_enabled",
  ].filter((k) => thresholds[k as keyof PolicyThresholds] as boolean).length;

  return (
    <DashboardShell>
      <div className="space-y-6">
        {fetchError && (
          <div className="rounded-md border border-yellow-500/30 bg-yellow-500/10 px-4 py-3 text-sm text-yellow-600">
            {fetchError}
          </div>
        )}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Policies</h2>
            <p className="text-muted-foreground">
              Configure detection thresholds and enable/disable governance checks per application.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <Badge variant="outline" className="text-xs">
              {enabledCount}/11 checks enabled
            </Badge>
            {policyVersion !== null && (
              <Badge variant="secondary" className="text-xs">
                v{policyVersion}
              </Badge>
            )}
            <Select
              value={selectedApp}
              onValueChange={(v) => setSelectedApp(v)}
              options={apps.map((app) => ({ value: app.id, label: app.name }))}
              size="md"
            />
          </div>
        </div>

        {/* Regulatory Profile Selector — only shown for Agent-Internal (App1) */}
        {profiles.length > 0 && selectedApp === "10000000-0000-0000-0000-000000000002" && (
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Regulatory Profile</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground mb-3">
                Click a profile to view and edit its thresholds. Save updates that profile independently.
              </p>
              {selectedProfile && (
                <div className="flex items-center gap-2 mb-3">
                  <p className="text-xs text-primary font-medium">
                    Editing: {profiles.find(p => p.id === selectedProfile)?.name ?? selectedProfile}
                  </p>
                  <button
                    onClick={deselectProfile}
                    className="text-[10px] text-muted-foreground underline hover:text-foreground"
                  >
                    Back to app thresholds
                  </button>
                </div>
              )}
              {!selectedProfile && activeProfile && (
                <p className="text-xs text-emerald-500 mb-3 font-medium">
                  App using profile: {profiles.find(p => p.id === activeProfile)?.name ?? activeProfile}
                </p>
              )}
              <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                {profiles.map((p) => {
                  const isSelected = selectedProfile === p.id;
                  const isActive = activeProfile === p.id;
                  return (
                    <button
                      key={p.id}
                      onClick={() => selectProfile(p)}
                      className={`text-left rounded-md border p-3 transition-all ${
                        isSelected
                          ? "border-primary bg-primary/5 shadow-lg shadow-primary/20 ring-2 ring-primary/40"
                          : isActive
                          ? "border-emerald-400 bg-emerald-50/5"
                          : "border-border hover:border-primary/50 hover:bg-muted/50"
                      }`}
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">{p.name}</span>
                        <Badge variant="outline" className="text-[10px]">
                          {p.risk_appetite}
                        </Badge>
                        {isSelected && (
                          <Badge variant="default" className="text-[9px] ml-auto">Editing</Badge>
                        )}
                        {isActive && !isSelected && (
                          <Badge variant="secondary" className="text-[9px] ml-auto bg-emerald-100 text-emerald-700">Enforced</Badge>
                        )}
                      </div>
                      <p className="text-[11px] text-muted-foreground mt-1 line-clamp-2">
                        {p.description}
                      </p>
                      <div className="flex flex-wrap gap-1 mt-2">
                        {p.regulations.slice(0, 3).map((r) => (
                          <Badge key={r} variant="secondary" className="text-[9px]">
                            {r}
                          </Badge>
                        ))}
                      </div>
                    </button>
                  );
                })}
              </div>
            </CardContent>
          </Card>
        )}

        {/* Policy Effectiveness Stats */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Policy Effectiveness</CardTitle>
            <Badge variant="outline" className="text-xs">
              Blocks &amp; escalations per check (7d)
            </Badge>
          </CardHeader>
          <CardContent>
            {policyStats.length === 0 ? (
              <p className="text-xs text-muted-foreground py-4 text-center">
                No verdicts recorded in the selected window yet. Send traffic through the proxy to see policy effectiveness.
              </p>
            ) : (
              <div className="space-y-2">
                <div className="grid grid-cols-[1fr_3.5rem_4rem_3rem_3rem_3.5rem] gap-x-2 text-[10px] uppercase tracking-wider text-muted-foreground/60 font-mono pb-1 border-b border-border/50">
                  <span>Check</span>
                  <span className="text-right">Blocked</span>
                  <span className="text-right">Escalated</span>
                  <span className="text-right">Edited</span>
                  <span className="text-right">Passed</span>
                  <span className="text-right">FP rate</span>
                </div>
                {policyStats.map((s) => {
                  const resolved = s.confirmed + s.overridden + s.dismissed;
                  const fpRate = resolved > 0 ? (s.overridden + s.dismissed) / resolved : 0;
                  const fpHigh = resolved >= 3 && fpRate > 0.3;
                  return (
                    <div
                      key={`${s.check_name}-${s.axis}`}
                      className="grid grid-cols-[1fr_3.5rem_4rem_3rem_3rem_3.5rem] gap-x-2 items-center text-xs py-1.5 rounded-md hover:bg-muted/30 px-1 -mx-1"
                    >
                      <div className="flex items-center gap-2 min-w-0">
                        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${s.blocks + s.escalates > 0 ? "bg-orange-400" : "bg-emerald-400"}`} />
                        <span className="font-medium truncate">{s.check_name}</span>
                        <Badge variant="outline" className="text-[9px] px-1 py-0 hidden sm:inline-flex">{s.axis}</Badge>
                      </div>
                      <span className={`text-right font-mono tabular-nums ${s.blocks > 0 ? "text-red-400 font-semibold" : "text-muted-foreground/40"}`}>
                        {s.blocks}
                      </span>
                      <span className={`text-right font-mono tabular-nums ${s.escalates > 0 ? "text-orange-400 font-semibold" : "text-muted-foreground/40"}`}>
                        {s.escalates}
                      </span>
                      <span className={`text-right font-mono tabular-nums ${s.edits > 0 ? "text-blue-400" : "text-muted-foreground/40"}`}>
                        {s.edits}
                      </span>
                      <span className="text-right font-mono tabular-nums text-muted-foreground/50">
                        {s.passes}
                      </span>
                      <span
                        className={`text-right font-mono tabular-nums ${
                          resolved === 0
                            ? "text-muted-foreground/30"
                            : fpHigh
                              ? "text-red-400 font-semibold"
                              : "text-emerald-400"
                        }`}
                        title={resolved > 0 ? `${s.confirmed} confirmed / ${s.overridden} overridden / ${s.dismissed} dismissed` : "No resolutions yet"}
                      >
                        {resolved === 0 ? "—" : `${Math.round(fpRate * 100)}%`}
                      </span>
                    </div>
                  );
                })}
                <p className="text-[10px] text-muted-foreground/50 pt-2 border-t border-border/50">
                  <strong>FP rate</strong> = (overridden + dismissed) / total resolved. A high FP rate means this check is flagging too much good content — consider raising thresholds or disabling the check.
                </p>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Thresholds */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Thresholds</CardTitle>
            <Badge variant="outline" className="text-xs">
              Performance &amp; Cost
            </Badge>
          </CardHeader>
          <CardContent className="space-y-4">
            <ThresholdSlider
              label="Block Threshold"
              description="Block response if confidence exceeds this"
              value={thresholds.block_threshold}
              min={0.5}
              max={1.0}
              step={0.05}
              onChange={(v) => setThresholds((t) => ({ ...t, block_threshold: v }))}
              action="Block"
            />
            <ThresholdSlider
              label="Escalate Threshold"
              description="Escalate to human review if confidence exceeds this"
              value={thresholds.escalate_threshold}
              min={0.3}
              max={0.9}
              step={0.05}
              onChange={(v) => setThresholds((t) => ({ ...t, escalate_threshold: v }))}
              action="Escalate"
            />
            <div className="grid grid-cols-2 gap-4">
              <div>
                <div className="flex items-center justify-between mb-1.5">
                  <label className="text-sm font-semibold tracking-tight text-foreground/90">Max Tokens Per Request</label>
                  <span className="text-sm font-mono tabular-nums text-muted-foreground">
                    {thresholds.max_tokens_per_request.toLocaleString()}
                  </span>
                </div>
                <div className="relative">
                  <div className="absolute inset-y-0 left-0 right-0 my-auto h-1.5 rounded-full bg-border/60" />
                  <div
                    className="absolute inset-y-0 left-0 my-auto h-1.5 rounded-full bg-gradient-to-r from-blue-500 to-blue-400 transition-all duration-150"
                    style={{ width: `${((thresholds.max_tokens_per_request - 10) / (16000 - 10)) * 100}%` }}
                  />
                  <input
                    type="range"
                    min={10}
                    max={16000}
                    step={10}
                    value={thresholds.max_tokens_per_request}
                    onChange={(e) =>
                      setThresholds((t) => ({ ...t, max_tokens_per_request: parseInt(e.target.value) }))
                    }
                    className="relative z-10 w-full h-6 appearance-none bg-transparent cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-blue-500 [&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-150 [&::-webkit-slider-thumb]:hover:scale-125 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-white [&::-moz-range-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-moz-range-thumb]:border-2 [&::-moz-range-thumb]:border-blue-500 [&::-moz-range-thumb]:appearance-none"
                  />
                </div>
                <div className="flex justify-between text-[10px] text-muted-foreground/50 mt-1 font-mono">
                  <span>10</span>
                  <span>16,000</span>
                </div>
              </div>
              <div>
                <div className="flex items-center justify-between mb-1.5">
                  <label className="text-sm font-semibold tracking-tight text-foreground/90">Max Retries</label>
                  <span className="text-sm font-mono tabular-nums text-muted-foreground">
                    {thresholds.retry_max_count}
                  </span>
                </div>
                <div className="relative">
                  <div className="absolute inset-y-0 left-0 right-0 my-auto h-1.5 rounded-full bg-border/60" />
                  <div
                    className="absolute inset-y-0 left-0 my-auto h-1.5 rounded-full bg-gradient-to-r from-blue-500 to-blue-400 transition-all duration-150"
                    style={{ width: `${((thresholds.retry_max_count - 1) / (10 - 1)) * 100}%` }}
                  />
                  <input
                    type="range"
                    min={1}
                    max={10}
                    step={1}
                    value={thresholds.retry_max_count}
                    onChange={(e) =>
                      setThresholds((t) => ({ ...t, retry_max_count: parseInt(e.target.value) }))
                    }
                    className="relative z-10 w-full h-6 appearance-none bg-transparent cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-blue-500 [&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-150 [&::-webkit-slider-thumb]:hover:scale-125 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-white [&::-moz-range-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-moz-range-thumb]:border-2 [&::-moz-range-thumb]:border-blue-500 [&::-moz-range-thumb]:appearance-none"
                  />
                </div>
                <div className="flex justify-between text-[10px] text-muted-foreground/50 mt-1 font-mono">
                  <span>1</span>
                  <span>10</span>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Fast-Path Governance Checks */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Fast-Path Checks</CardTitle>
            <Badge variant="outline" className="text-xs bg-green-500/10 text-green-500 border-green-500/20">
              &lt;10ms latency
            </Badge>
          </CardHeader>
          <CardContent className="space-y-3">
            <p className="text-xs text-muted-foreground">
              Synchronous checks that run on every response before it reaches the client. Cannot be disabled individually — disable at the engine level.
            </p>

            <GuardrailToggle
              label="Unsafe Content Detection"
              description="Blocks responses containing known unsafe keywords (hacking, malware, weapons, self-harm). Uses deterministic keyword matching with context awareness."
              provider="Fast-Path Engine"
              enabled={thresholds.unsafe_content_enabled}
              onChange={() => toggle("unsafe_content_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Secret Detection"
              description="Detects and auto-redacts secrets in responses (API keys, SSNs, emails, credit cards). Uses regex pattern matching with entropy scoring."
              provider="Fast-Path Engine"
              enabled={thresholds.secret_detection_enabled}
              onChange={() => toggle("secret_detection_enabled")}
              disabled={!isEditable}
            />
          </CardContent>
        </Card>

        {/* Shadow-Path Governance Checks */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Shadow-Path Checks</CardTitle>
            <Badge variant="outline" className="text-xs bg-orange-500/10 text-orange-500 border-orange-500/20">
              Async (1-2s)
            </Badge>
          </CardHeader>
          <CardContent className="space-y-3">
            <p className="text-xs text-muted-foreground">
              Expensive checks that run asynchronously after the response is delivered. Verdicts appear in the dashboard ~1-2s later.
            </p>

            <GuardrailToggle
              label="Prompt Injection Detection"
              description="Detects adversarial inputs (jailbreaking, instruction override, role hijacking, prompt extraction). Uses 3-layer detection: pattern matching (25+ known patterns), structural heuristics, and encoding analysis."
              provider="Pure Rust"
              enabled={thresholds.prompt_injection_enabled}
              onChange={() => toggle("prompt_injection_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Hallucination Detection"
              description="Compares model response against provided context to detect unsupported claims using DeepEval's LLM-as-a-judge approach."
              provider="DeepEval"
              enabled={thresholds.hallucination_detection_enabled}
              onChange={() => toggle("hallucination_detection_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Groundedness Scoring"
              description="Evaluates whether response claims are supported by the provided context. Scores each claim and flags low-groundedness responses."
              provider="NLI Model"
              enabled={thresholds.groundedness_enabled}
              onChange={() => toggle("groundedness_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Verbosity Detection"
              description="Detects excessively verbose responses that inflate token costs. Uses response-to-prompt ratio and information density scoring."
              provider="Shadow Analysis"
              enabled={thresholds.verbosity_enabled}
              onChange={() => toggle("verbosity_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Semantic PII Detection"
              description="Quasi-identifier / re-identification heuristic. Runs as a fallback: when Presidio or the judge reports PII on the same response, the stronger detector supersedes this keyword heuristic."
              provider="NER Model"
              enabled={thresholds.semantic_pii_enabled}
              onChange={() => toggle("semantic_pii_enabled")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Decision-Model Judge"
              description="Laya/Jev scores every governance question in one batched forward pass and returns calibrated probabilities. Shadow-path only: it scores, it never decides — the decision engine applies the thresholds. Requires DECISION_JUDGE=laya|jev on the server; this switch can only opt an app out."
              provider="Laya / Jev"
              enabled={thresholds.decision_judge_enabled}
              onChange={() => toggle("decision_judge_enabled")}
              disabled={!isEditable}
            />
          </CardContent>
        </Card>

        {/* Guardrails Sidecar Checks */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Guardrails Sidecar</CardTitle>
            <Badge variant="outline" className="text-xs bg-blue-500/10 text-blue-500 border-blue-500/20">
              Python (Presidio + LLM Guard)
            </Badge>
          </CardHeader>
          <CardContent className="space-y-3">
            <p className="text-xs text-muted-foreground">
              External Python sidecar checks using Microsoft Presidio and LLM Guard frameworks. Runs on both input prompts and output responses.
            </p>

            <GuardrailToggle
              label="PII Detection (Presidio)"
              description="Microsoft Presidio-based PII detection on responses. Scans for SSNs, emails, credit cards, phone numbers, and named entities."
              provider="Microsoft Presidio"
              enabled={thresholds.pii_detection}
              onChange={() => toggle("pii_detection")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Toxicity Detection"
              description="LLM Guard-based toxicity scanning on both input prompts and output responses. Detects hate speech, violence, self-harm content."
              provider="LLM Guard"
              enabled={thresholds.toxicity_detection}
              onChange={() => toggle("toxicity_detection")}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Bias Detection"
              description="Scans input prompts for demographic stereotypes, cultural prejudices, and unfair treatment across protected groups."
              provider="LLM Guard"
              enabled={thresholds.bias_detection}
              onChange={() => toggle("bias_detection")}
              disabled={!isEditable}
            />
          </CardContent>
        </Card>

        {/* Save button */}
        <div className="flex items-center gap-3">
          {isEditable ? (
            <button
              onClick={handleSave}
              disabled={saving}
              className="rounded-lg bg-emerald-500 px-5 py-2.5 text-sm font-semibold text-white shadow-[0_1px_2px_rgba(0,0,0,0.15),0_0_12px_rgba(16,185,129,0.15)] hover:bg-emerald-400 hover:shadow-[0_1px_4px_rgba(0,0,0,0.2),0_0_16px_rgba(16,185,129,0.25)] active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-200"
            >
              {saving ? "Saving..." : selectedProfile ? `Save to ${profiles.find(p => p.id === selectedProfile)?.name ?? "Profile"}` : "Save Policy"}
            </button>
          ) : (
            <span className="rounded-lg border border-border px-5 py-2.5 text-sm text-muted-foreground/60">
              Read-only (admin required)
            </span>
          )}
          {saved && (
            <span className="flex items-center gap-1.5 text-sm text-emerald-400 font-medium">
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
              Policy saved successfully
            </span>
          )}
        </div>
      </div>

    </DashboardShell>
  );
}

function ThresholdSlider({
  label,
  description,
  value,
  min,
  max,
  step,
  onChange,
  action,
}: {
  label: string;
  description: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
  action: string;
}) {
  return (
    <div className="group">
      <div className="flex items-center justify-between mb-1.5">
        <label className="text-sm font-semibold tracking-tight text-foreground/90">{label}</label>
        <div className="flex items-center gap-2">
          <span className="text-sm font-mono tabular-nums text-muted-foreground">
            {value.toFixed(2)}
          </span>
          <Badge
            variant="outline"
            className={`text-[10px] px-1.5 py-0 ${action === "Block" ? "text-red-400 border-red-500/30" : "text-orange-400 border-orange-500/30"}`}
          >
            {action}
          </Badge>
        </div>
      </div>
      <p className="text-[11px] text-muted-foreground/70 mb-3">{description}</p>
      <div className="relative">
        <div className="absolute inset-y-0 left-0 right-0 my-auto h-1.5 rounded-full bg-border/60" />
        <div
          className="absolute inset-y-0 left-0 my-auto h-1.5 rounded-full bg-gradient-to-r from-emerald-500 to-emerald-400 transition-all duration-150"
          style={{ width: `${((value - min) / (max - min)) * 100}%` }}
        />
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => onChange(parseFloat(e.target.value))}
          className="relative z-10 w-full h-6 appearance-none bg-transparent cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-emerald-500 [&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-150 [&::-webkit-slider-thumb]:hover:scale-125 [&::-webkit-slider-thumb]:focus-visible:outline-none [&::-webkit-slider-thumb]:focus-visible:ring-2 [&::-webkit-slider-thumb]:focus-visible:ring-emerald-500/40 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-white [&::-moz-range-thumb]:shadow-[0_1px_4px_rgba(0,0,0,0.25)] [&::-moz-range-thumb]:border-2 [&::-moz-range-thumb]:border-emerald-500 [&::-moz-range-thumb]:appearance-none"
        />
      </div>
      <div className="flex justify-between text-[10px] text-muted-foreground/50 mt-1.5 font-mono">
        <span>{min}</span>
        <span>{max}</span>
      </div>
    </div>
  );
}

function GuardrailToggle({
  label,
  description,
  provider,
  enabled,
  onChange,
  disabled,
}: {
  label: string;
  description: string;
  provider: string;
  enabled: boolean;
  onChange: () => void;
  disabled?: boolean;
}) {
  return (
    <div
      className={`group relative flex items-center gap-4 rounded-xl border p-4 transition-all duration-300 ease-in-out ${
        enabled
          ? "border-emerald-500/25 bg-emerald-500/[0.04] shadow-[0_0_0_1px_rgba(16,185,129,0.08)]"
          : "border-border/60 bg-card hover:border-border hover:bg-accent/30"
      } ${disabled ? "pointer-events-none opacity-50" : ""}`}
    >
      {/* Status indicator dot */}
      <div
        className={`mt-0.5 h-2 w-2 shrink-0 rounded-full transition-colors duration-300 ${
          enabled ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.5)]" : "bg-muted-foreground/30"
        }`}
      />

      <div className="flex-1 space-y-1.5">
        <div className="flex items-center gap-2.5">
          <span className="text-sm font-semibold tracking-tight text-foreground/90 group-hover:text-foreground transition-colors">
            {label}
          </span>
          <Badge
            variant="outline"
            className={`text-[10px] font-mono px-1.5 py-0 ${
              enabled
                ? "border-emerald-500/25 text-emerald-500/80"
                : "border-border text-muted-foreground/60"
            }`}
          >
            {provider}
          </Badge>
        </div>
        <p className="text-[11px] leading-relaxed text-muted-foreground/70 max-w-[520px]">
          {description}
        </p>
      </div>

      {/* Toggle switch */}
      <button
        type="button"
        role="switch"
        aria-checked={enabled}
        aria-label={`${label} toggle`}
        disabled={disabled}
        onClick={onChange}
        className={`relative inline-flex h-[26px] w-[46px] shrink-0 cursor-pointer items-center rounded-full transition-all duration-300 ease-[cubic-bezier(0.4,0,0.2,1)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-40 ${
          enabled
            ? "bg-emerald-500 shadow-[0_0_12px_rgba(16,185,129,0.25),inset_0_1px_0_rgba(255,255,255,0.15)]"
            : "bg-border/80 shadow-inner"
        }`}
      >
        <span
          className={`pointer-events-none inline-block h-[18px] w-[18px] rounded-full shadow-lg transition-all duration-300 ease-[cubic-bezier(0.4,0,0.2,1)] ${
            enabled
              ? "translate-x-[24px] bg-white shadow-[0_1px_3px_rgba(0,0,0,0.2),0_0_8px_rgba(16,185,129,0.3)]"
              : "translate-x-[3px] bg-muted-foreground/50 shadow-[0_1px_2px_rgba(0,0,0,0.15)]"
          }`}
        />
      </button>
    </div>
  );
}
