"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchApi } from "@/lib/api";

interface AppInfo {
  id: string;
  name: string;
  data_governance_level?: string;
}

interface AppContextValue {
  apps: AppInfo[];
  selectedAppId: string;
  setSelectedAppId: (id: string) => void;
  selectedAppName: string;
}

const AppContext = createContext<AppContextValue>({
  apps: [],
  selectedAppId: "all",
  setSelectedAppId: () => {},
  selectedAppName: "All Applications",
});

export function AppProvider({ children }: { children: ReactNode }) {
  const [selectedAppId, setSelectedAppId] = useState("all");

  const { data: apps = [] } = useQuery<AppInfo[]>({
    queryKey: ["apps-list"],
    queryFn: () => fetchApi<AppInfo[]>("/api/v1/apps"),
    staleTime: 60_000,
    retry: 3,
  });

  const selectedAppName =
    selectedAppId === "all"
      ? "All Applications"
      : apps.find((a) => a.id === selectedAppId)?.name ?? selectedAppId;

  return (
    <AppContext.Provider value={{ apps, selectedAppId, setSelectedAppId, selectedAppName }}>
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  return useContext(AppContext);
}
