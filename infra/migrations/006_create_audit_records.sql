CREATE TABLE audit_records (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    call_id      UUID NOT NULL REFERENCES intercepted_calls(id),
    verdict_id   UUID NOT NULL REFERENCES verdicts(id),
    app_id       UUID NOT NULL REFERENCES apps(id),
    action_taken TEXT NOT NULL CHECK (action_taken IN ('pass', 'edit', 'block', 'escalate')),
    prev_hash    TEXT NOT NULL,
    record_hash  TEXT NOT NULL,
    metadata     JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_app_time ON audit_records(app_id, created_at DESC);
CREATE INDEX idx_audit_action ON audit_records(action_taken, created_at DESC);
CREATE INDEX idx_audit_call ON audit_records(call_id);
CREATE INDEX idx_audit_hash ON audit_records(record_hash);
