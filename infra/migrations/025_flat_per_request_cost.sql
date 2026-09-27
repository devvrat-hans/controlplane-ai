-- Billing switched to a flat $1.00 per request (controlplane_common::COST_PER_REQUEST_USD).
-- Reprice historical ledger rows so cost_entries agrees with new entries.
-- The dashboard derives spend from intercepted_calls (request count x price),
-- so every historical request is already priced at $1 there without a backfill.
UPDATE cost_entries SET cost_usd = 1.0 WHERE cost_usd <> 1.0;
