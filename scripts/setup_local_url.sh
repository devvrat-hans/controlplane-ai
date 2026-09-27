#!/usr/bin/env bash
#
# ControlPlane.ai — branded local URL
#
# Makes the dashboard reachable at a branded, port-free, loopback-only name
# instead of http://localhost:3000:
#
#   http://controlplane-ai.rtxcore        (default — one-time hosts alias)
#
# The default name lives outside the reserved *.localhost TLD (RFC 6761), so it
# is NOT resolved automatically. Run the `hosts` command once (needs sudo) to add
# "127.0.0.1 controlplane-ai.rtxcore" to /etc/hosts. The only other requirement is
# that something listens on port 80 — the `nginx` service started automatically by
# `docker-compose up --build`.
#
# Commands:
#   ./scripts/setup_local_url.sh              # check + report what is serving :80
#   ./scripts/setup_local_url.sh verify       # curl the branded URL and its routes
#   ./scripts/setup_local_url.sh hosts NAME   # install the /etc/hosts alias (one
#                                             # sudo); defaults to the branded name
#   ./scripts/setup_local_url.sh start-host   # host nginx fallback (needs sudo:
#                                             # binds :80) for the three-terminal
#                                             # mode where docker compose is not up
#   ./scripts/setup_local_url.sh stop-host
#   ./scripts/setup_local_url.sh status
#
# With Colima this was verified to work end to end without root: Colima forwards
# container port 80 to the host, and macOS allows that forward. The host nginx
# path is only a fallback.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NGINX_CONF="$REPO_ROOT/infra/nginx/nginx.conf"
PID_FILE="/tmp/controlplane-nginx.pid"
DEFAULT_NAME="controlplane-ai.rtxcore"

RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; DIM=$'\033[2m'; NC=$'\033[0m'
info()  { printf '%s==>%s %s\n' "$GREEN" "$NC" "$*"; }
warn()  { printf '%s==>%s %s\n' "$YELLOW" "$NC" "$*"; }
die()   { printf '%s==>%s %s\n' "$RED" "$NC" "$*" >&2; exit 1; }

nginx_running() {
  [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE" 2>/dev/null)" 2>/dev/null
}

port80_pids() {
  command -v lsof >/dev/null 2>&1 && lsof -nP -iTCP:80 -sTCP:LISTEN -t 2>/dev/null || true
}

docker_names() {
  docker ps --format '{{.Names}}' 2>/dev/null || true
}

port80_listener() {
  # "host-nginx" | "docker-nginx" | "other" | "" (nothing listening)
  local pids
  pids="$(port80_pids)"
  [[ -z "$pids" ]] && return 0
  if nginx_running && grep -qx "$(cat "$PID_FILE" 2>/dev/null)" <<<"$pids"; then
    echo "host-nginx"
  elif docker_names | grep -q '^controlplane-nginx$'; then
    echo "docker-nginx"
  else
    echo "other"
  fi
}

flush_dns() {
  [[ "$(uname -s)" == "Darwin" ]] || return 0
  sudo dscacheutil -flushcache 2>/dev/null || true
  sudo killall -HUP mDNSResponder 2>/dev/null || true
}

# True when $NAME already maps to loopback in /etc/hosts. Only meaningful for
# names outside *.localhost, which the OS resolves for free.
name_in_hosts() {
  awk -v h="$1" '$1 == "127.0.0.1" { for (i = 2; i <= NF; i++) if ($i == h) found = 1 } END { exit found ? 0 : 1 }' /etc/hosts
}

cmd="${1:-}"
arg="${2:-}"
case "$cmd" in
  "" | status | verify | hosts | start-host | stop-host) ;;
  *) die "Unknown command '$cmd'. See the header of this script for usage." ;;
esac
NAME="${arg:-${HOSTNAME_LOCAL:-$DEFAULT_NAME}}"

case "$cmd" in
  status)
    case "$(port80_listener)" in
      docker-nginx) info "Port 80: served by the Docker nginx container (controlplane-nginx)" ;;
      host-nginx)   info "Port 80: served by host nginx (pid $(cat "$PID_FILE"))" ;;
      other)        warn "Port 80: served by another process — the branded URL may not work" ;;
      *)            warn "Port 80: nothing listening — start the stack: docker-compose up --build" ;;
    esac
    printf '\n%sBrowsable now:%s\n' "$DIM" "$NC"
    printf '  http://%s\n' "$DEFAULT_NAME"
    printf '\n%s/etc/hosts loopback aliases:%s\n' "$DIM" "$NC"
    grep -E '^[[:space:]]*127\.0\.0\.1[[:space:]]' /etc/hosts || true
    if [[ "$DEFAULT_NAME" == *.localhost ]] || name_in_hosts "$DEFAULT_NAME"; then
      info "'$DEFAULT_NAME' resolves to loopback."
    else
      warn "'$DEFAULT_NAME' is outside *.localhost and not in /etc/hosts yet."
      printf '   Add it once:  ./scripts/setup_local_url.sh hosts %s\n' "$DEFAULT_NAME"
    fi
    exit 0
    ;;

  verify)
    rc=0
    check() {
      local label="$1" url="$2" verb="${3:-GET}" body="${4:-}"
      local code
      if [[ -n "$body" ]]; then
        code="$(curl -s -o /dev/null -w '%{http_code}' -X "$verb" "$url" \
          -H 'Content-Type: application/json' -d "$body" --max-time 60 || echo 000)"
      else
        code="$(curl -s -o /dev/null -w '%{http_code}' -X "$verb" "$url" --max-time 15 || echo 000)"
      fi
      if [[ "$code" =~ ^2 ]]; then
        printf '  %sOK%s   %-28s %s\n' "$GREEN" "$NC" "$label" "$code"
      else
        printf '  %sFAIL%s %-28s %s\n' "$RED" "$NC" "$label" "$code"; rc=1
      fi
    }
    info "Checking http://$NAME and its routes…"
    check "dashboard (frontend)"  "http://$NAME/" GET
    check "gateway health"        "http://$NAME/health" GET
    check "dashboard API"         "http://$NAME/api/v1/stats/overview" GET
    check "verdict stream (SSE)"  "http://$NAME/api/v1/verdicts/recent" GET
    check "governance proxy"      "http://$NAME/v1/messages" POST \
      '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What is 2+2?"}],"max_tokens":20}'
    [[ $rc -eq 0 ]] && info "All routes healthy — open http://$NAME" || die "Some routes failed (see above)."
    exit $rc
    ;;

  hosts)
    if name_in_hosts "$NAME"; then
      info "'$NAME' already resolves to 127.0.0.1 in /etc/hosts"
    else
      info "Adding '127.0.0.1 $NAME' to /etc/hosts (needs sudo)…"
      echo "127.0.0.1 $NAME" | sudo tee -a /etc/hosts >/dev/null
      info "Added — the name resolves to loopback, so traffic stays on this machine."
      flush_dns
    fi
    printf 'Browse to http://%s\n' "$NAME"
    exit 0
    ;;

  stop-host)
    if nginx_running; then
      [[ -f "$NGINX_CONF" ]] || die "Missing $NGINX_CONF"
      info "Stopping host nginx…"
      sudo nginx -c "$NGINX_CONF" -s stop
      info "Stopped. (The Docker nginx service still owns :80 if the stack is up.)"
    else
      warn "Host nginx is not running. To stop the Docker proxy: docker-compose stop nginx"
    fi
    exit 0
    ;;

  start-host)
    [[ -f "$NGINX_CONF" ]] || die "Missing $NGINX_CONF"
    command -v nginx >/dev/null 2>&1 || die "nginx not found (brew install nginx)"
    case "$(port80_listener)" in
      docker-nginx)
        info "Port 80 is already served by the Docker nginx container — nothing to start."
        printf 'Browse to http://%s\n' "$DEFAULT_NAME"
        exit 0
        ;;
      other)
        die "Port 80 is held by another process. Stop it first, or use the Docker nginx service."
        ;;
    esac
    info "Checking nginx config syntax…"
    sudo nginx -t -c "$NGINX_CONF"
    if nginx_running; then
      info "Reloading host nginx…"
      sudo nginx -c "$NGINX_CONF" -s reload
    else
      info "Starting host nginx on port 80 (needs sudo)…"
      sudo nginx -c "$NGINX_CONF"
    fi
    flush_dns
    info "Ready → http://$DEFAULT_NAME"
    exit 0
    ;;
esac

# Default action: report, and start the Docker proxy if the stack is up but the
# nginx service was created before it existed.
info "Branded local URL: http://$DEFAULT_NAME"
if [[ "$DEFAULT_NAME" != *.localhost ]] && ! name_in_hosts "$DEFAULT_NAME"; then
  warn "'$DEFAULT_NAME' is not in /etc/hosts yet — the URL will not resolve."
  printf '   Add it once:  ./scripts/setup_local_url.sh hosts %s\n' "$DEFAULT_NAME"
fi
case "$(port80_listener)" in
  docker-nginx)
    info "Already served by the Docker nginx container."
    ;;
  host-nginx)
    info "Already served by host nginx."
    ;;      "")
    warn "Nothing is listening on port 80."
    if docker_names | grep -q '^controlplane-frontend$'; then
      info "The stack is up but without the nginx service — starting it…"
      # Non-fatal: compose needs DOCKER_HOST set in this shell (see README).
      docker-compose up -d --no-deps nginx || warn "Could not start it here — run: docker-compose up -d --no-deps nginx"
      sleep 3
    else
      printf 'Start the stack first:  docker-compose up --build\n'
      printf '%s(If docker reports a missing socket, export DOCKER_HOST — see README §5.)%s\n' "$DIM" "$NC"
    fi
    ;;
  *)
    warn "Port 80 is held by another process; the branded URL may not reach this stack."
    lsof -nP -iTCP:80 -sTCP:LISTEN 2>/dev/null | sed -n '1,3p' || true
    ;;
esac
printf '\nRun ./scripts/setup_local_url.sh verify to check every route.\n'
