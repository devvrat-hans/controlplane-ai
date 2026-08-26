-- Reviewer Override Learning Store (Round 2 feedback loop)
-- Every escalation resolution is captured here so that similar future calls
-- can retrieve "precedents" — past reviewer decisions on similar content.
--
-- Similarity uses pg_trgm trigram matching on request/response excerpts
-- (available in standard PostgreSQL contrib, no external extension needed).
-- This is the retrieval ("R") side of the retraining loop.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE IF NOT EXISTS reviewer_overrides (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    escalation_id     UUID REFERENCES escalation_cases(id),
    call_id           UUID NOT NULL REFERENCES intercepted_calls(id),
    app_id            UUID NOT NULL REFERENCES apps(id),
    verdict_id        UUID,
    axis              TEXT NOT NULL,
    check_name        TEXT,
    model_outcome     TEXT NOT NULL,
    model_confidence  REAL,
    reviewer_action   TEXT NOT NULL CHECK (reviewer_action IN ('confirm', 'override', 'dismiss')),
    reviewer_reason   TEXT,
    request_excerpt   TEXT,
    response_excerpt  TEXT,
    session_id        UUID,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_reviewer_overrides_app_axis ON reviewer_overrides(app_id, axis);
CREATE INDEX idx_reviewer_overrides_response_trgm ON reviewer_overrides USING gin (response_excerpt gin_trgm_ops);
CREATE INDEX idx_reviewer_overrides_request_trgm ON reviewer_overrides USING gin (request_excerpt gin_trgm_ops);

-- Backfill: seed precedents from already-resolved escalation cases so the
-- learning loop has data on day one of the demo.
INSERT INTO reviewer_overrides (
    escalation_id, call_id, app_id, verdict_id, axis, check_name,
    model_outcome, model_confidence, reviewer_action, reviewer_reason,
    request_excerpt, response_excerpt, created_at
)
SELECT
    e.id, e.call_id, e.app_id, e.verdict_id, e.axis, v.check_name,
    'escalate', e.confidence, e.resolution, e.resolution_reason,
    LEFT(ic.request_payload::text, 2000),
    LEFT(ic.response_payload::text, 2000),
    COALESCE(e.resolved_at, e.created_at)
FROM escalation_cases e
JOIN intercepted_calls ic ON ic.id = e.call_id
LEFT JOIN verdicts v ON v.id = e.verdict_id
WHERE e.status = 'resolved' AND e.resolution IS NOT NULL
ON CONFLICT DO NOTHING;
