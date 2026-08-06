import { defineStore } from "pinia";

import { api, setServerConfig, type PrRecord, type SnapshotRecord, type WorkspaceRecord } from "@/api/client";

interface ServerState {
  connected: boolean;
  baseUrl: string;
  token: string;
  prs: PrRecord[];
  workspaces: WorkspaceRecord[];
  snapshots: SnapshotRecord[];
  error: string | null;
  loading: boolean;
}

export const useServerStore = defineStore("server", {
  state: (): ServerState => ({
    connected: false,
    baseUrl: localStorage.getItem("noa.server_base_url") || "http://127.0.0.1:3000",
    token: localStorage.getItem("noa.server_token") || "",
    prs: [],
    workspaces: [],
    snapshots: [],
    error: null,
    loading: false,
  }),
  actions: {
    configure(base: string, apiToken: string) {
      setServerConfig(base, apiToken);
      this.baseUrl = base;
      this.token = apiToken;
      this.connected = true;
      this.error = null;
    },
    async refreshAll() {
      this.loading = true;
      this.error = null;
      try {
        const [prs, workspaces, snapshots] = await Promise.all([
          api.listPrs(),
          api.listWorkspaces(),
          api.listSnapshots(),
        ]);
        this.prs = prs;
        this.workspaces = workspaces;
        this.snapshots = snapshots;
        this.connected = true;
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e);
        this.connected = false;
      } finally {
        this.loading = false;
      }
    },
    async refreshPrs() {
      try {
        this.prs = await api.listPrs();
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e);
      }
    },
  },
});
