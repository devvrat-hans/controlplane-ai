import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export default function LiveStreamPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Live Stream</h2>
            <p className="text-muted-foreground">
              Real-time verdict feed from the proxy.
            </p>
          </div>
          <div className="flex items-center gap-2">
            <div className="h-2 w-2 rounded-full bg-green-500 animate-pulse" />
            <span className="text-sm text-muted-foreground">Connected</span>
          </div>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Verdict Stream
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              <VerdictEntry
                time="12:34:56"
                axis="responsibility"
                outcome="edit"
                reason="Detected potential API key in response"
                path="fast"
              />
              <VerdictEntry
                time="12:34:52"
                axis="cost"
                outcome="pass"
                reason="Token count within budget"
                path="fast"
              />
              <VerdictEntry
                time="12:34:48"
                axis="performance"
                outcome="escalate"
                reason="Low groundedness score (0.42)"
                path="shadow"
              />
            </div>
            <p className="mt-4 text-center text-sm text-muted-foreground">
              Waiting for live data from SSE endpoint...
            </p>
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

function VerdictEntry({
  time,
  axis,
  outcome,
  reason,
  path,
}: {
  time: string;
  axis: string;
  outcome: string;
  reason: string;
  path: string;
}) {
  const outcomeColor = {
    pass: "bg-green-500/10 text-green-500 border-green-500/20",
    edit: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
    block: "bg-red-500/10 text-red-500 border-red-500/20",
    escalate: "bg-orange-500/10 text-orange-500 border-orange-500/20",
  }[outcome] ?? "";

  return (
    <div className="flex items-center gap-3 rounded-md border border-border p-3">
      <span className="text-xs font-mono text-muted-foreground">{time}</span>
      <Badge variant="outline" className="text-xs capitalize">
        {axis}
      </Badge>
      <Badge className={`text-xs capitalize ${outcomeColor}`}>
        {outcome}
      </Badge>
      <Badge variant="secondary" className="text-xs">
        {path}
      </Badge>
      <span className="flex-1 text-sm text-muted-foreground truncate">
        {reason}
      </span>
    </div>
  );
}
