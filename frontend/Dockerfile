# syntax=docker/dockerfile:1

FROM node:22-alpine AS builder

WORKDIR /app

RUN corepack enable

# Optional registry mirror for slow/unreliable links to registry.npmjs.org, e.g.
#   NPM_REGISTRY=https://registry.npmmirror.com docker compose up --build
ARG NPM_REGISTRY=https://registry.npmjs.org/
ENV npm_config_registry=$NPM_REGISTRY

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./

# The pnpm store lives in a BuildKit cache mount so an interrupted build resumes
# from what was already downloaded instead of restarting at zero. The fetch-*
# flags raise pnpm's 60s default timeout, which was aborting installs on slow links.
RUN --mount=type=cache,id=pnpm-store,target=/pnpm/store \
    pnpm install \
      --store-dir /pnpm/store \
      --prefer-offline \
      --no-frozen-lockfile \
      --ignore-scripts \
      --network-concurrency 4 \
      --fetch-timeout 600000 \
      --fetch-retries 5 \
      --fetch-retry-mintimeout 20000 \
      --fetch-retry-maxtimeout 120000

COPY . .

ARG NEXT_PUBLIC_API_URL=http://localhost:8080
ENV NEXT_PUBLIC_API_URL=$NEXT_PUBLIC_API_URL
ARG NEXT_PUBLIC_PROXY_URL=http://localhost:8900
ENV NEXT_PUBLIC_PROXY_URL=$NEXT_PUBLIC_PROXY_URL
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

# --- Runtime stage ---
FROM node:22-alpine

WORKDIR /app

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV PORT=3000
ENV HOSTNAME=0.0.0.0

COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
COPY --from=builder /app/node_modules ./node_modules

EXPOSE 3000

CMD ["node", "server.js"]
