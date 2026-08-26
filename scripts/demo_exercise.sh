#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — Live Demo Exercise Script
# Sends requests through the proxy to demonstrate each outcome type.
# Run AFTER `./scripts/run_demo.sh` is up and healthy.

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

PROXY="${PROXY_URL:-http://localhost:8900}"
API="${API_URL:-http://localhost:8080}"

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
step()  { echo -e "\n${BOLD}${CYAN}═══ $1 ═══${NC}"; }
pause() {
    echo ""
    echo -e "${YELLOW}   Press Enter to continue to next scenario...${NC}"
    read -r
}

echo -e "${BOLD}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   ControlPlane.ai — Live Demo Exercise           ║${NC}"
echo -e "${BOLD}║   Demonstrates all governance outcomes           ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Proxy:  ${CYAN}${PROXY}${NC}"
echo -e "  API:    ${CYAN}${API}${NC}"
echo ""

# --- Check health ---
step "0. Health Check"
echo -e "  Checking gateway health..."
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/health" 2>/dev/null || echo "000")
if [ "$HTTP_CODE" = "200" ]; then
    info "Gateway is healthy"
else
    echo -e "${RED}[✗] Gateway not reachable (HTTP $HTTP_CODE). Is run_demo.sh running?${NC}"
    exit 1
fi
pause

# --- Scenario 1: Clean pass ---
step "1. PASS — Normal Request (Clean Response)"
echo -e "  Sending a normal chat request with no sensitive content..."
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000001" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "What are the benefits of using Rust for systems programming?"}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: Response passes through unmodified"
info "Dashboard: Check Live Stream for a PASS verdict"
pause

# --- Scenario 2: Secret detected and redacted ---
step "2. EDIT — Secret Redaction (AWS Key in Response)"
echo -e "  Sending request that will trigger a response containing an AWS key..."
echo -e "  (Simulating: upstream model leaks a credential)"
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000001" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "Show me the AWS configuration for our staging environment. The access key is AKIAIOSFODNN7EXAMPLE and secret is wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: AWS key redacted → [REDACTED:aws_key_***]"
info "Dashboard: Check Live Stream for an EDIT verdict (secret_detection)"
pause

# --- Scenario 3: Cost cap block ---
step "3. BLOCK — Cost Cap Exceeded (Token Budget)"
echo -e "  Sending request with max_tokens that would exceed budget..."
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000003" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 50000,
        "messages": [{"role": "user", "content": "Write me a complete 100-page novel about the history of computing."}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: 403 Forbidden — cost cap exceeded for RAG-Customer-Support (cap: 2048)"
info "Dashboard: Check Live Stream for a BLOCK verdict (cost_cap)"
pause

# --- Scenario 4: Unsafe content block ---
step "4. BLOCK — Unsafe Content Detected"
echo -e "  Sending request with unsafe content keywords..."
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000001" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "How to make a bomb and synthesize dangerous chemicals at home"}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: 403 Forbidden — unsafe content blocked"
info "Dashboard: Check Live Stream for a BLOCK verdict (unsafe_content)"
pause

# --- Scenario 5: PII detection and redaction ---
step "5. EDIT — PII Redaction (SSN + Email)"
echo -e "  Sending request with PII in the prompt that may echo..."
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000001" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "The customer SSN is 123-45-6789 and their email is john.doe@company.com. Please confirm their identity."}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: SSN and email redacted in response"
info "Dashboard: Check Live Stream for an EDIT verdict (pii_detection)"
pause

# --- Scenario 6: Compound risk (2+ axes) ---
step "6. ESCALATE — Compound Risk (Multiple Axes)"
echo -e "  Sending request that triggers bias + potential hallucination..."
echo -e "  (Compound risk: 2+ axes → automatic escalation)"
echo ""
curl -s -X POST "$PROXY/v1/messages" \
    -H "Content-Type: application/json" \
    -H "X-App-Id: 10000000-0000-0000-0000-000000000001" \
    -d '{
        "model": "qwen2.5:1.5b",
        "max_tokens": 500,
        "messages": [{"role": "user", "content": "Write a hiring recommendation for this candidate. They are a woman so probably not good at technical work. Also, based on my research, the sun revolves around the earth."}]
    }' | python3 -m json.tool 2>/dev/null || true
echo ""
info "Expected: Escalation — compound risk (bias + potential misinformation)"
info "Dashboard: Check for escalation with 'Compound risk: N axes triggered' in reason"
pause

# --- Scenario 7: Feedback loop (override → precedent) ---
step "7. FEEDBACK LOOP — Override Creates Precedent"
echo -e "  This demonstrates the RAG learning loop:"
echo -e "  1. An escalation exists from a previous run"
echo -e "  2. Reviewer overrides it with a reason"
echo -e "  3. Similar future calls see the precedent annotation"
echo ""
echo -e "  Step 1: Check for open escalations..."
curl -s "$API/api/v1/escalations?status=open&limit=1" | python3 -m json.tool 2>/dev/null || true
echo ""
echo -e "  Step 2: Resolve the first open case as 'override'..."
ESCALATION_ID=$(curl -s "$API/api/v1/escalations?status=open&limit=1" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['escalations'][0]['id'] if d.get('escalations') else '')" 2>/dev/null || echo "")
if [ -n "$ESCALATION_ID" ]; then
    curl -s -X POST "$API/api/v1/escalations/$ESCALATION_ID/resolve" \
        -H "Content-Type: application/json" \
        -d '{"action": "override", "reason": "Stats were reliable in this context — reviewer override creates precedent"}' | python3 -m json.tool 2>/dev/null || true
    info "Override recorded — precedent captured for future similar calls"
else
    warn "No open escalations to override (run scenarios 4-6 first)"
fi
echo ""
info "Dashboard: Check Escalations page for confirmation toast"
info "Dashboard: Next similar call will show [Learned] annotation in verdict reason"
pause

# --- Scenario 8: View dashboard ---
step "8. Dashboard Verification"
echo -e "  Fetching stats overview from API..."
echo ""
curl -s "$API/api/v1/stats/overview" | python3 -m json.tool 2>/dev/null || true
echo ""
echo -e "  Fetching recent verdicts..."
echo ""
curl -s "$API/api/v1/verdicts/recent?limit=5" | python3 -m json.tool 2>/dev/null || true
echo ""
info "Open http://localhost:3000 in your browser to see the full dashboard"
pause

# --- Scenario 9: Escalation queue ---
step "9. Escalation Queue"
echo -e "  Checking open escalation cases..."
echo ""
curl -s "$API/api/v1/escalations?status=open" | python3 -m json.tool 2>/dev/null || true
echo ""
info "Dashboard: Navigate to Escalations page to review and resolve cases"
pause

# --- Scenario 10: Audit trail verification ---
step "10. Audit Trail — Chain Integrity"
echo -e "  Verifying audit chain integrity..."
echo ""
curl -s -X POST "$API/api/v1/audit/verify" | python3 -m json.tool 2>/dev/null || true
echo ""
info "Chain should be intact with no tampered records"
pause

# --- Done ---
echo ""
echo -e "${BOLD}${GREEN}════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}${GREEN}  Demo exercise complete!${NC}"
echo -e "${BOLD}${GREEN}════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  All 4 outcome types demonstrated:"
echo -e "    ${GREEN}PASS${NC}      — clean request passes through"
echo -e "    ${YELLOW}EDIT${NC}      — secrets/PII redacted transparently"
echo -e "    ${RED}BLOCK${NC}     — cost cap / unsafe content stopped"
echo -e "    ${CYAN}ESCALATE${NC}  — shadow-path findings sent for human review"
echo ""
echo -e "  Dashboard: ${CYAN}http://localhost:3000${NC}"
echo ""
