import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export default function AuditPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Audit Trail</h2>
            <p className="text-muted-foreground">
              Tamper-evident, hash-chained log of all decisions.
            </p>
          </div>
          <button className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-muted transition-colors">
            Verify Chain Integrity
          </button>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Recent Records
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-2">
              <AuditRow
                hash="a3f2c1..."
                action="edit"
                reason="PII redacted from response"
                time="12:34:56"
              />
              <AuditRow
                hash="b7e4d9..."
                action="block"
                reason="Unsafe content detected"
                time="12:33:12"
              />
              <AuditRow
                hash="c1a8f3..."
                action="escalate"
                reason="Bias score above threshold"
                time="12:31:45"
              />
              <AuditRow
                hash="d5b2e7..."
                action="pass"
                reason="All checks passed"
                time="12:30:22"
              />
            </div>
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

function AuditRow({
  hash,
  action,
  reason,
  time,
}: {
  hash: string;
  action: string;
  reason: string;
  time: string;
}) {
  const actionColor = {
    pass: "bg-green-500/10 text-green-500",
    edit: "bg-yellow-500/10 text-yellow-500",
    block: "bg-red-500/10 text-red-500",
    escalate: "bg-orange-500/10 text-orange-500",
  }[action] ?? "";

  return (
    <div className="flex items-center gap-3 rounded-md border border-border p-3">
      <span className="text-xs font-mono text-muted-foreground">{hash}</span>
      <Badge className={`text-xs capitalize ${actionColor}`}>{action}</Badge>
      <span className="flex-1 text-sm">{reason}</span>
      <span className="text-xs text-muted-foreground">{time}</span>
    </div>
  );
}
