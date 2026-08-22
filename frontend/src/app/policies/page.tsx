"use client";

import { useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useUser, canEditPolicies } from "@/lib/auth";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8081";

interface PolicyThresholds {
  block_threshold: number;
  escalate_threshold: number;
  max_tokens_per_request: number;
  retry_max_count: number;
  pii_detection: boolean;
  toxicity_detection: boolean;
  bias_detection: boolean;
}

const DEFAULT_THRESHOLDS: PolicyThresholds = {
  block_threshold: 0.9,
  escalate_threshold: 0.6,
  max_tokens_per_request: 4000,
  retry_max_count: 3,
  pii_detection: true,
  toxicity_detection: true,
  bias_detection: true,
};

const APPS = [
  { id: "10000000-0000-0000-0000-000000000001", name: "ChatBot-Prod" },
  { id: "10000000-0000-0000-0000-000000000002", name: "Agent-Internal" },
  { id: "10000000-0000-0000-0000-000000000003", name: "RAG-Customer-Support" },
];

export default function PoliciesPage() {
  const user = useUser();
  const isEditable = user ? canEditPolicies(user.role) : false;
  const [selectedApp, setSelectedApp] = useState(APPS[0].id);
  const [thresholds, setThresholds] = useState<PolicyThresholds>(DEFAULT_THRESHOLDS);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const loadPolicy = async (appId: string) => {
    try {
      const res = await fetch(`${API_BASE}/api/v1/policies/${appId}`);
      if (res.ok) {
        const data = await res.json();
        if (data.policies && data.policies.length > 0) {
          const config = data.policies[0].config;
          if (config) {
            setThresholds({
              block_threshold: config.block_threshold ?? DEFAULT_THRESHOLDS.block_threshold,
              escalate_threshold: config.escalate_threshold ?? DEFAULT_THRESHOLDS.escalate_threshold,
              max_tokens_per_request: config.max_tokens_per_request ?? DEFAULT_THRESHOLDS.max_tokens_per_request,
              retry_max_count: config.retry_max_count ?? DEFAULT_THRESHOLDS.retry_max_count,
              pii_detection: config.pii_detection ?? DEFAULT_THRESHOLDS.pii_detection,
              toxicity_detection: config.toxicity_detection ?? DEFAULT_THRESHOLDS.toxicity_detection,
              bias_detection: config.bias_detection ?? DEFAULT_THRESHOLDS.bias_detection,
            });
            return;
          }
        }
      }
    } catch { /* fall through to defaults */ }
    setThresholds(DEFAULT_THRESHOLDS);
  };

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);

    try {
      const res = await fetch(`${API_BASE}/api/v1/policies/${selectedApp}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(thresholds),
      });
      if (res.ok) {
        setSaved(true);
        setTimeout(() => setSaved(false), 3000);
      }
    } catch {
      // API might not be running
    } finally {
      setSaving(false);
    }
  };

  return (
    <DashboardShell>
      <div className="space-y-6">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Policies</h2>
            <p className="text-muted-foreground">
              Configure detection thresholds per application.
            </p>
          </div>
          <select
            value={selectedApp}
            onChange={(e) => { setSelectedApp(e.target.value); loadPolicy(e.target.value); }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            {APPS.map((app) => (
              <option key={app.id} value={app.id}>
                {app.name}
              </option>
            ))}
          </select>
        </div>

        {/* Performance Axis */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">
              Performance Axis
            </CardTitle>
            <Badge variant="outline" className="text-xs">
              Groundedness, Verbosity
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
              onChange={(v) =>
                setThresholds((t) => ({ ...t, block_threshold: v }))
              }
              action="Block"
            />
            <ThresholdSlider
              label="Escalate Threshold"
              description="Escalate to human review if confidence exceeds this"
              value={thresholds.escalate_threshold}
              min={0.3}
              max={0.9}
              step={0.05}
              onChange={(v) =>
                setThresholds((t) => ({ ...t, escalate_threshold: v }))
              }
              action="Escalate"
            />
          </CardContent>
        </Card>

        {/* Cost Axis */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">Cost Axis</CardTitle>
            <Badge variant="outline" className="text-xs">
              Token Budget, Retry Detection
            </Badge>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-sm font-medium">
                  Max Tokens Per Request
                </label>
                <span className="text-sm font-mono text-muted-foreground">
                  {thresholds.max_tokens_per_request.toLocaleString()}
                </span>
              </div>
              <input
                type="range"
                min={500}
                max={16000}
                step={500}
                value={thresholds.max_tokens_per_request}
                onChange={(e) =>
                  setThresholds((t) => ({
                    ...t,
                    max_tokens_per_request: parseInt(e.target.value),
                  }))
                }
                className="w-full accent-primary"
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-1">
                <span>500</span>
                <span>16,000</span>
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-sm font-medium">
                  Max Retries Before Escalation
                </label>
                <span className="text-sm font-mono text-muted-foreground">
                  {thresholds.retry_max_count}
                </span>
              </div>
              <input
                type="range"
                min={1}
                max={10}
                step={1}
                value={thresholds.retry_max_count}
                onChange={(e) =>
                  setThresholds((t) => ({
                    ...t,
                    retry_max_count: parseInt(e.target.value),
                  }))
                }
                className="w-full accent-primary"
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-1">
                <span>1</span>
                <span>10</span>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Responsibility Axis */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium">
              Responsibility Axis
            </CardTitle>
            <Badge variant="outline" className="text-xs">
              PII, Toxicity, Bias
            </Badge>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-xs text-muted-foreground">
              Enable or disable governance guardrails powered by open-source frameworks.
            </p>

            <GuardrailToggle
              label="PII Detection"
              description="Detect and redact personally identifiable information (SSN, emails, credit cards, names, etc.)"
              provider="Microsoft Presidio"
              enabled={thresholds.pii_detection}
              onChange={(v) => setThresholds((t) => ({ ...t, pii_detection: v }))}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Toxicity Detection"
              description="Block toxic, harmful, or unsafe content (hate speech, violence, self-harm)"
              provider="LLM Guard"
              enabled={thresholds.toxicity_detection}
              onChange={(v) => setThresholds((t) => ({ ...t, toxicity_detection: v }))}
              disabled={!isEditable}
            />

            <GuardrailToggle
              label="Bias Detection"
              description="Flag demographic stereotypes, cultural prejudices, or unfair treatment across protected groups"
              provider="LLM Guard"
              enabled={thresholds.bias_detection}
              onChange={(v) => setThresholds((t) => ({ ...t, bias_detection: v }))}
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
              className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {saving ? "Saving..." : "Save Policy"}
            </button>
          ) : (
            <span className="rounded-md border border-border px-4 py-2 text-sm text-muted-foreground">
              Read-only (admin required)
            </span>
          )}
          {saved && (
            <span className="text-sm text-green-500">
              Policy saved successfully
            </span>
          )}
        </div>

        {/* Policy version history placeholder */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Version History
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-2">
              <VersionRow version={3} date="2026-08-19" change="Updated block threshold to 0.9" active />
              <VersionRow version={2} date="2026-08-18" change="Added retry detection limit" />
              <VersionRow version={1} date="2026-08-17" change="Initial policy creation" />
            </div>
          </CardContent>
        </Card>
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
    <div>
      <div className="flex items-center justify-between mb-1">
        <label className="text-sm font-medium">{label}</label>
        <div className="flex items-center gap-2">
          <span className="text-sm font-mono text-muted-foreground">
            {value.toFixed(2)}
          </span>
          <Badge
            variant="outline"
            className={`text-[10px] ${action === "Block" ? "text-red-500 border-red-500/30" : "text-orange-500 border-orange-500/30"}`}
          >
            {action}
          </Badge>
        </div>
      </div>
      <p className="text-xs text-muted-foreground mb-2">{description}</p>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-full accent-primary"
      />
      <div className="flex justify-between text-[10px] text-muted-foreground mt-1">
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
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className={`flex items-center justify-between rounded-lg border p-4 ${enabled ? "border-primary/30 bg-primary/5" : "border-border"}`}>
      <div className="space-y-1 flex-1 mr-4">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{label}</span>
          <Badge variant="outline" className="text-[10px] font-mono">
            {provider}
          </Badge>
        </div>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={enabled}
        disabled={disabled}
        onClick={() => onChange(!enabled)}
        className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 ${
          enabled ? "bg-primary" : "bg-muted"
        }`}
      >
        <span
          className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow-lg ring-0 transition-transform duration-200 ease-in-out ${
            enabled ? "translate-x-5" : "translate-x-0"
          }`}
        />
      </button>
    </div>
  );
}

function VersionRow({
  version,
  date,
  change,
  active,
}: {
  version: number;
  date: string;
  change: string;
  active?: boolean;
}) {
  return (
    <div className="flex items-center justify-between rounded-md border border-border p-3">
      <div className="flex items-center gap-3">
        <span className="text-xs font-mono text-muted-foreground">
          v{version}
        </span>
        <span className="text-sm">{change}</span>
      </div>
      <div className="flex items-center gap-2">
        {active && (
          <Badge className="text-[10px] bg-green-500/10 text-green-500 border-green-500/20">
            Active
          </Badge>
        )}
        <span className="text-xs text-muted-foreground">{date}</span>
      </div>
    </div>
  );
}
