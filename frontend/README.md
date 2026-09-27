This is a [Next.js](https://nextjs.org) project bootstrapped with [`create-next-app`](https://nextjs.org/docs/app/api-reference/cli/create-next-app).

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
# or
bun dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

## Branded local URL (RTXCore ControlPlane AI)

The browser tab title is `RTXCore ControlPlane AI` (set in `src/app/layout.tsx`).
The dashboard is also reachable at a branded, **port-free**, loopback-only name.
It sits outside the reserved `*.localhost` TLD, so it needs a one-time
`/etc/hosts` alias (one `sudo`, ever):

```bash
# from the repo root, with the stack up
docker-compose up --build          # Linux/Windows: docker compose up --build
./scripts/setup_local_url.sh hosts controlplane-ai.rtxcore   # one sudo, once
open http://controlplane-ai.rtxcore
```

The name resolves to `127.0.0.1` via that single hosts line, so it is local by
construction. The nginx service (config:
`infra/nginx/docker.conf`) terminates port 80 and forwards to the compose
services:

| Path      | Upstream                          |
| --------- | --------------------------------- |
| `/`       | Next.js dashboard (`frontend:3000`) |
| `/api/`   | Dashboard API, REST + SSE (`gateway:8080`) |
| `/health` | Gateway health (`gateway:8080`)   |
| `/v1/`    | Governance proxy (`gateway:8900`) |

`next.config.ts` lists `controlplane-ai.rtxcore` in
`allowedDevOrigins`, which Next.js requires before it will serve **dev** traffic
from a non-`localhost` origin — needed when you run `pnpm dev` behind the proxy,
after the hosts alias above.

Verify or inspect it from the repo root:

```bash
./scripts/setup_local_url.sh verify    # checks all five routes
./scripts/setup_local_url.sh status    # who owns :80?
```

Because the proxy serves the API and the dashboard on one origin, the app can
also drop `localhost` from its own network calls — set in `frontend/.env.local`:

```bash
NEXT_PUBLIC_API_URL=http://controlplane-ai.rtxcore
NEXT_PUBLIC_PROXY_URL=http://controlplane-ai.rtxcore
```

Optional: with the `localhost` defaults the dashboard still works, since the
dashboard API's CORS layer accepts any origin. The in-app branding and tab title
never depend on this step.

For a different name outside `*.localhost` (e.g. `rtxcore.internal`), add a
one-time alias: `./scripts/setup_local_url.sh hosts rtxcore.internal`.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

This project uses [`next/font`](https://nextjs.org/docs/app/building-your-application/optimizing/fonts) to automatically optimize and load [Geist](https://vercel.com/font), a new font family for Vercel.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js) - your feedback and contributions are welcome!

## Deploy on Vercel

The easiest way to deploy your Next.js app is to use the [Vercel Platform](https://vercel.com/new?utm_medium=default-template&filter=next.js&utm_source=create-next-app&utm_campaign=create-next-app-readme) from the creators of Next.js.

Check out our [Next.js deployment documentation](https://nextjs.org/docs/app/building-your-application/deploying) for more details.
