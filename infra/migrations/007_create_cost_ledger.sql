CREATE TABLE cost_ledger (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id              UUID NOT NULL REFERENCES apps(id),
    window_start        TIMESTAMPTZ NOT NULL,
    window_end          TIMESTAMPTZ NOT NULL,
    total_tokens_input  BIGINT NOT NULL DEFAULT 0,
    total_tokens_output BIGINT NOT NULL DEFAULT 0,
    total_cost_cents    BIGINT NOT NULL DEFAULT 0,
    request_count       INT NOT NULL DEFAULT 0,
    avg_tokens_per_req  INT,
    baseline_avg        REAL,
    baseline_deviation  REAL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(app_id, window_start)
);

CREATE INDEX idx_cost_app_window ON cost_ledger(app_id, window_start DESC);
