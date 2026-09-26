# Laya decision-model sidecar — Jev-compatible System One HTTP server.
#
# `laya-serve` exposes POST /v1/systemone using the same request/response shape as
# TypeSafe's Jev, so the Rust shadow-analysis client can talk to either backend by
# changing a base URL. See docs/analysis/laya-integration-plan.md.
#
# The judge is OFF by default: set DECISION_JUDGE=laya and LAYA_URL=http://laya:8000
# on the gateway to enable it. With DECISION_JUDGE=off this container is never called.

FROM python:3.11-slim

WORKDIR /app

# `laya[serve]` pulls the serve extras (HTTP server). Pinned to the 0.3 line, which is
# the release that ships the Jev-compatible endpoint and the faster checkpoint loading.
RUN pip install --no-cache-dir "laya[serve]>=0.3.11"

# Optional: uncomment to bake the checkpoints into the image (~808 MB English,
# ~647 MB multilingual). This makes the image much larger but removes the cold-start
# model download. Off by default so `docker compose up --build` stays fast.
# RUN python -c "import laya; laya.load('convaiinnovations/laya')"

EXPOSE 8000

# Keep the checkpoints resident so a language switch costs only detection (<1 ms)
# instead of a 7-10 s reload.
ENV LAYA_PRELOAD=1
ENV LAYA_DEVICE=cpu

# Set LAYA_API_KEY to require `Authorization: Bearer <key>`; left unset for local demo.
# ENV LAYA_API_KEY=

CMD ["laya-serve"]
