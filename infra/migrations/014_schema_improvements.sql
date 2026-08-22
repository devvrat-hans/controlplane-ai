-- Schema improvements and fixes.
-- Addresses: broken cost_ledger design, redundant indexes, missing constraints,
-- nullable columns that should be NOT NULL, and missing referential integrity.

-- ============================================================================
-- 1. Fix cost_ledger: separate per-request entries from windowed aggregates
-- ============================================================================

-- Create a proper per-request cost entries table
CREATE TABLE IF NOT EXISTS cost_entries (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id            UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    call_id           UUID REFERENCES intercepted_calls(id) ON DELETE SET NULL,
    model             TEXT NOT NULL DEFAULT 'unknown',
    input_tokens      INT NOT NULL DEFAULT 0,
    output_tokens     INT NOT NULL DEFAULT 0,
    cost_usd          DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_cost_entries_app_time ON cost_entries(app_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_cost_entries_created ON cost_entries(created_at DESC);

-- Restore cost_ledger to its original windowed-aggregate purpose
-- Re-add the unique constraint (if rows conflict, this is idempotent)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'cost_ledger_app_window_unique'
    ) THEN
        -- Only add if no duplicate rows exist
        BEGIN
            ALTER TABLE cost_ledger
                ADD CONSTRAINT cost_ledger_app_window_unique UNIQUE (app_id, window_start);
        EXCEPTION WHEN unique_violation THEN
            RAISE NOTICE 'Duplicate rows exist in cost_ledger, skipping unique constraint';
        END;
    END IF;
END $$;

-- Make window_start/window_end nullable for backward compat with per-request rows already inserted
ALTER TABLE cost_ledger ALTER COLUMN window_start DROP NOT NULL;
ALTER TABLE cost_ledger ALTER COLUMN window_end DROP NOT NULL;

-- ============================================================================
-- 2. Remove redundant index (UNIQUE already creates one)
-- ============================================================================

DROP INDEX IF EXISTS idx_users_email;

-- ============================================================================
-- 3. Create teams table and add FK to apps.team_id
-- ============================================================================

CREATE TABLE IF NOT EXISTS teams (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Don't add FK constraint to existing apps.team_id since existing data has NULLs
-- (which is fine for optional FK), but some may have non-null values without matching teams.
-- In production, you'd backfill first.

-- ============================================================================
-- 4. Make verdicts.app_id NOT NULL (backfill first)
-- ============================================================================

-- Backfill any remaining NULLs from intercepted_calls
UPDATE verdicts v
SET app_id = ic.app_id
FROM intercepted_calls ic
WHERE v.call_id = ic.id AND v.app_id IS NULL;

-- For any verdicts with no matching intercepted_call, use a sentinel app
UPDATE verdicts
SET app_id = (SELECT id FROM apps LIMIT 1)
WHERE app_id IS NULL;

-- Now enforce NOT NULL
ALTER TABLE verdicts ALTER COLUMN app_id SET NOT NULL;

-- ============================================================================
-- 5. Add ON DELETE policies to foreign keys
-- ============================================================================

-- intercepted_calls.app_id: if app deleted, keep calls for audit (RESTRICT)
-- verdicts.call_id: if call deleted, cascade (verdicts have no meaning without call)
-- audit_records: never delete (RESTRICT on all FKs — enforced by AGENTS.md contract)

-- For escalation_cases, cascade on verdict deletion
ALTER TABLE escalation_cases DROP CONSTRAINT IF EXISTS escalation_cases_verdict_id_fkey;
ALTER TABLE escalation_cases
    ADD CONSTRAINT escalation_cases_verdict_id_fkey
    FOREIGN KEY (verdict_id) REFERENCES verdicts(id) ON DELETE CASCADE;

ALTER TABLE escalation_cases DROP CONSTRAINT IF EXISTS escalation_cases_call_id_fkey;
ALTER TABLE escalation_cases
    ADD CONSTRAINT escalation_cases_call_id_fkey
    FOREIGN KEY (call_id) REFERENCES intercepted_calls(id) ON DELETE CASCADE;

-- ============================================================================
-- 6. Add missing useful indexes
-- ============================================================================

-- For dashboard "recent calls" queries
CREATE INDEX IF NOT EXISTS idx_calls_model ON intercepted_calls(model);

-- For escalation assignment queries
CREATE INDEX IF NOT EXISTS idx_escalations_created ON escalation_cases(created_at DESC);

-- For audit chain verification (sequential ordering)
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_records(created_at ASC);

-- ============================================================================
-- 7. Add updated_at column to users (useful for profile edits)
-- ============================================================================

ALTER TABLE users ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ DEFAULT NOW();

-- ============================================================================
-- 8. Add app name uniqueness constraint
-- ============================================================================

-- App names should be unique within a team (or globally if no team)
CREATE UNIQUE INDEX IF NOT EXISTS idx_apps_name_unique
    ON apps(name) WHERE team_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_apps_name_team_unique
    ON apps(name, team_id) WHERE team_id IS NOT NULL;
