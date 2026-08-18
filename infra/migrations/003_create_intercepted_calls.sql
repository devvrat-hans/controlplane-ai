CREATE TABLE intercepted_calls (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    correlation_id      UUID NOT NULL UNIQUE,
    app_id              UUID NOT NULL REFERENCES apps(id),
    model               TEXT NOT NULL,
    request_payload     JSONB,
    response_payload    JSONB,
    token_count_input   INT,
    token_count_output  INT,
    upstream_latency_ms INT,
    fast_path_latency_ms INT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_calls_app_time ON intercepted_calls(app_id, created_at DESC);
CREATE INDEX idx_calls_correlation ON intercepted_calls(correlation_id);
CREATE INDEX idx_calls_created ON intercepted_calls(created_at DESC);
