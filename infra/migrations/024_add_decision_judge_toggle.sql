-- Make the decision-model judge toggle explicit in every active policy row
-- (Laya integration plan §10 / §11.2, migration `024`).
--
-- The shadow toggle store already defaults `decision_judge` to ON when the key is
-- absent, so this backfill changes NO behaviour. It exists so that:
--   1. the Policies page can display the toggle's real value for every app, and
--   2. a policy rewrite/round-trip cannot silently drop the key.
--
-- Note the semantics: this per-app flag can only opt an app OUT of the judge. The
-- process-level master switch remains the `DECISION_JUDGE` env var, which is `off`
-- by default — so writing `true` here does not turn the judge on.

-- 1. Rows that already have a `checks` object: add the key if missing.
UPDATE policies
SET threshold_config = jsonb_set(
        threshold_config,
        '{checks}',
        (threshold_config -> 'checks') || jsonb_build_object('decision_judge_enabled', TRUE),
        TRUE
    ),
    updated_at = NOW()
WHERE is_active = TRUE
  AND axis IN ('performance', 'responsibility')
  AND threshold_config ? 'checks'
  AND NOT ((threshold_config -> 'checks') ? 'decision_judge_enabled');

-- 2. Legacy rows with no `checks` object at all: create one.
UPDATE policies
SET threshold_config = jsonb_set(
        threshold_config,
        '{checks}',
        jsonb_build_object('decision_judge_enabled', TRUE),
        TRUE
    ),
    updated_at = NOW()
WHERE is_active = TRUE
  AND axis IN ('performance', 'responsibility')
  AND NOT (threshold_config ? 'checks');
