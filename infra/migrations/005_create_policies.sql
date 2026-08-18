CREATE TABLE policies (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id           UUID NOT NULL REFERENCES apps(id),
    axis             TEXT NOT NULL CHECK (axis IN ('performance', 'cost', 'responsibility')),
    threshold_config JSONB NOT NULL,
    version          INT NOT NULL DEFAULT 1,
    is_active        BOOLEAN NOT NULL DEFAULT TRUE,
    updated_by       UUID REFERENCES users(id),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(app_id, axis, version)
);

CREATE INDEX idx_policies_app_axis ON policies(app_id, axis) WHERE is_active = TRUE;
