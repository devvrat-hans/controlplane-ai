-- API Keys table for real authentication and per-key analytics
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_prefix TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{proxy:read,proxy:write}',
    status TEXT NOT NULL DEFAULT 'active',
    app_id UUID REFERENCES apps(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status);

-- Add api_key_id to intercepted_calls for per-key tracking
ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS api_key_id UUID REFERENCES api_keys(id);
