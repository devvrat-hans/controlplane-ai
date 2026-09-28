# Laya decision-model sidecar — Jev-compatible System One HTTP server.
#
# `laya-serve` exposes POST /v1/systemone using the same request/response shape as
# TypeSafe's Jev, so the Rust shadow-analysis client can talk to either backend by
# changing a base URL. See docs/analysis/laya-integration-plan.md.
#
# The gateway uses it for the hallucination check whenever LAYA_URL is set (docker
# compose sets LAYA_URL=http://laya:8000); unset LAYA_URL to skip that check.

FROM python:3.11-slim

WORKDIR /app

# CPU-only PyTorch first. The default PyPI wheel bundles the NVIDIA CUDA stack
# (cuDNN, cuBLAS, … several GB) that this CPU container (LAYA_DEVICE=cpu) never uses;
# with torch already satisfied, laya's dependency resolution keeps this build.
RUN pip install --no-cache-dir --index-url https://download.pytorch.org/whl/cpu torch

# `laya[serve]` pulls the serve extras (HTTP server). Pinned to the 0.3 line, which is
# the release that ships the Jev-compatible endpoint and the faster checkpoint loading.
RUN pip install --no-cache-dir "laya[serve]>=0.3.11"

# Optional: uncomment to bake the checkpoints into the image (~808 MB English,
# ~647 MB multilingual). This makes the image much larger but removes the cold-start
# model download. Off by default so `docker compose up --build` stays fast.
# RUN python -c "import laya; laya.load('convaiinnovations/laya')"

# Healthcheck + watchdog: restarts a deadlocked laya-serve (see healthcheck.py).
COPY healthcheck.py /app/healthcheck.py

EXPOSE 8000

# Keep the checkpoints resident so a language switch costs only detection (<1 ms)
# instead of a 7-10 s reload.
ENV LAYA_PRELOAD=1
ENV LAYA_DEVICE=cpu

# Set LAYA_API_KEY to require `Authorization: Bearer <key>`; left unset for local demo.
# ENV LAYA_API_KEY=

CMD ["laya-serve"]
