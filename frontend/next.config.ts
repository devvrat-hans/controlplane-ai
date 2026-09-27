import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // Reach the dev server through the branded local name instead of a bare
  // `localhost:3000`. The name is outside the reserved *.localhost TLD, so it
  // needs a one-time /etc/hosts alias (see README, §5a):
  //   ./scripts/setup_local_url.sh hosts controlplane-ai.rtxcore
  // This list only tells Next.js to accept dev requests whose Origin is the
  // branded name — it does not affect production (`next start`).
  allowedDevOrigins: ["controlplane-ai.rtxcore"],
};

export default nextConfig;
