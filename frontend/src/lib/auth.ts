"use client";

import { useEffect, useState } from "react";

export interface User {
  email: string;
  role: "admin" | "reviewer" | "viewer";
  password?: string;
}

export function useUser(): User | null {
  const [user, setUser] = useState<User | null>(null);

  useEffect(() => {
    const stored = sessionStorage.getItem("cp-user");
    if (stored) {
      try {
        setUser(JSON.parse(stored));
      } catch {
        /* ignore */
      }
    }
  }, []);

  return user;
}

export function canEditPolicies(role: string): boolean {
  return role === "admin";
}

export function canResolveEscalations(role: string): boolean {
  return role === "admin" || role === "reviewer";
}

export function canAccessSettings(role: string): boolean {
  return role === "admin";
}
