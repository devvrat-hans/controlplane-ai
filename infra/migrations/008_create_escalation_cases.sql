CREATE TABLE escalation_cases (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    verdict_id        UUID NOT NULL REFERENCES verdicts(id),
    call_id           UUID NOT NULL REFERENCES intercepted_calls(id),
    app_id            UUID NOT NULL REFERENCES apps(id),
    status            TEXT NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open', 'in_review', 'resolved')),
    assigned_to       UUID REFERENCES users(id),
    resolution        TEXT CHECK (resolution IN ('confirm', 'override', 'dismiss')),
    resolution_reason TEXT,
    axis              TEXT NOT NULL,
    confidence        REAL NOT NULL,
    reason            TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at       TIMESTAMPTZ
);

CREATE INDEX idx_escalations_status ON escalation_cases(status, created_at DESC);
CREATE INDEX idx_escalations_app ON escalation_cases(app_id, status);
CREATE INDEX idx_escalations_assigned ON escalation_cases(assigned_to) WHERE status != 'resolved';
