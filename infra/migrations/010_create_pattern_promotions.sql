CREATE TABLE pattern_promotions (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    check_name       TEXT NOT NULL,
    pattern_key      TEXT NOT NULL,
    occurrence_count INT NOT NULL DEFAULT 0,
    promoted         BOOLEAN NOT NULL DEFAULT FALSE,
    promoted_at      TIMESTAMPTZ,
    first_seen_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sample_reason    TEXT NOT NULL,
    promoted_rule    JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(check_name, pattern_key)
);

CREATE INDEX idx_pattern_promotions_promoted ON pattern_promotions(promoted) WHERE promoted = TRUE;
