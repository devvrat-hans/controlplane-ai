#!/usr/bin/env bash
set -euo pipefail

# Run all SQL migrations in order against PostgreSQL.
# Usage: ./infra/migrate.sh [DATABASE_URL]

DB_URL="${1:-${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}}"
MIGRATIONS_DIR="$(dirname "$0")/migrations"

echo "Running migrations against: ${DB_URL%%@*}@..."
echo "---"

for migration in "$MIGRATIONS_DIR"/*.sql; do
    filename=$(basename "$migration")
    echo "  Applying: $filename"
    psql "$DB_URL" -f "$migration" -v ON_ERROR_STOP=1 --quiet
done

echo "---"
echo "All migrations applied successfully."
