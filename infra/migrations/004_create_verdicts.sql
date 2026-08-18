CREATE TABLE verdicts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    call_id     UUID NOT NULL REFERENCES intercepted_calls(id),
    axis        TEXT NOT NULL CHECK (axis IN ('performance', 'cost', 'responsibility')),
    path        TEXT NOT NULL CHECK (path IN ('fast', 'shadow')),
    outcome     TEXT NOT NULL CHECK (outcome IN ('pass', 'edit', 'block', 'escalate')),
    confidence  REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    reason      TEXT NOT NULL,
    check_name  TEXT NOT NULL,
    duration_ms INT,
    metadata    JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_verdicts_call ON verdicts(call_id);
CREATE INDEX idx_verdicts_outcome_time ON verdicts(outcome, created_at DESC);
CREATE INDEX idx_verdicts_axis ON verdicts(axis, created_at DESC);
CREATE INDEX idx_verdicts_created ON verdicts(created_at DESC);
