#!/usr/bin/env bash
# Preflight check — run all tests before demo or CI deployment.
# Usage: ./scripts/preflight.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

echo "=== ControlPlane.ai Preflight Checks ==="
echo ""

# 1. Rust workspace check
echo -e "${YELLOW}[1/5] cargo check --workspace${NC}"
if cargo check --workspace 2>&1; then
    echo -e "${GREEN}  ✓ Rust workspace compiles${NC}"
else
    echo -e "${RED}  ✗ Rust workspace compilation failed${NC}"
    exit 1
fi

# 2. Rust tests
echo -e "${YELLOW}[2/5] cargo test --workspace${NC}"
if cargo test --workspace 2>&1; then
    echo -e "${GREEN}  ✓ All Rust tests pass${NC}"
else
    echo -e "${RED}  ✗ Rust tests failed${NC}"
    exit 1
fi

# 3. Frontend TypeScript check
echo -e "${YELLOW}[3/5] TypeScript type check${NC}"
cd frontend
if npx tsc --noEmit 2>&1; then
    echo -e "${GREEN}  ✓ TypeScript compiles${NC}"
else
    echo -e "${RED}  ✗ TypeScript check failed${NC}"
    exit 1
fi

# 4. Frontend tests
echo -e "${YELLOW}[4/5] Frontend tests${NC}"
if npx vitest run 2>&1; then
    echo -e "${GREEN}  ✓ All frontend tests pass${NC}"
else
    echo -e "${RED}  ✗ Frontend tests failed${NC}"
    exit 1
fi

# 5. SQL migrations check
echo -e "${YELLOW}[5/5] Migration files present${NC}"
MIGRATION_COUNT=$(ls infra/migrations/*.sql 2>/dev/null | wc -l)
if [ "$MIGRATION_COUNT" -gt 0 ]; then
    echo -e "${GREEN}  ✓ ${MIGRATION_COUNT} migration files found${NC}"
else
    echo -e "${RED}  ✗ No migration files found${NC}"
    exit 1
fi

cd ..
echo ""
echo -e "${GREEN}=== All preflight checks passed ===${NC}"
