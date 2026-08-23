"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

export default function ApiKeysPage() {
  const router = useRouter();
  useEffect(() => {
    router.replace("/settings");
  }, [router]);
  return null;
}
