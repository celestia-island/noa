// Thin REST client for noa-server (/api/v1/*).
// The server requires a bearer token (NOA_API_TOKEN) on every request.

export interface PrRecord {
  number: number;
  repo: string;
  title: string;
  body: string;
  state: string;
  base: string;
  head: string;
  base_snapshot: string;
  author: string;
  created_at: number;
  merge_snapshot: string | null;
  metadata: Record<string, unknown> | null;
}

export interface WorkspaceRecord {
  name: string;
  head: string;
  base: string;
  agent_id: string | null;
  last_seq: number;
  created_at: number;
  updated_at: number;
}

export interface SnapshotRecord {
  id: string;
  tree_hash: string;
  parents: string[];
  workspace: string;
  author: string;
  timestamp: number;
  message: string;
}

export class NoaApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
    this.name = "NoaApiError";
  }
}

function baseUrl(): string {
  // Only accept a plausible absolute URL from storage/env. The `||` chain
  // alone would happily pass through the literal string "undefined" (a
  // legacy-poisoned localStorage value or an env var materialized from an
  // unset shell variable), and `fetch("undefined/api/...")` then resolves
  // against the page origin as `<origin>/undefined/...`.
  const candidates = [
    localStorage.getItem("noa.server_base_url"),
    import.meta.env.VITE_NOA_SERVER_URL as string | undefined,
  ];
  for (const candidate of candidates) {
    if (typeof candidate === "string" && /^https?:\/\//i.test(candidate)) {
      return candidate.replace(/\/+$/, "");
    }
  }
  return "http://127.0.0.1:3000";
}

function token(): string {
  return localStorage.getItem("noa.server_token") || "";
}

export function setServerConfig(base: string, apiToken: string): void {
  localStorage.setItem("noa.server_base_url", base.replace(/\/+$/, ""));
  localStorage.setItem("noa.server_token", apiToken);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  const t = token();
  if (t) {
    headers["Authorization"] = `Bearer ${t}`;
  }
  const resp = await fetch(`${baseUrl()}${path}`, {
    ...init,
    headers: { ...headers, ...(init?.headers ?? {}) },
  });
  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new NoaApiError(body.slice(0, 300) || `HTTP ${resp.status}`, resp.status);
  }
  if (resp.status === 204) {
    return undefined as T;
  }
  return (await resp.json()) as T;
}

export const api = {
  listPrs(params: { repo?: string; base?: string; state?: string } = {}): Promise<PrRecord[]> {
    const q = new URLSearchParams();
    if (params.repo) q.set("repo", params.repo);
    if (params.base) q.set("base", params.base);
    if (params.state) q.set("state", params.state);
    const qs = q.toString();
    return request<PrRecord[]>(`/api/v1/prs${qs ? `?${qs}` : ""}`);
  },
  getPr(number: number): Promise<PrRecord> {
    return request<PrRecord>(`/api/v1/prs/${number}`);
  },
  createPr(body: {
    title: string;
    body: string;
    base: string;
    head: string;
    author: string;
    repo?: string;
  }): Promise<PrRecord> {
    return request<PrRecord>("/api/v1/prs", { method: "POST", body: JSON.stringify(body) });
  },
  mergePr(number: number, squash: boolean): Promise<PrRecord> {
    return request<PrRecord>(`/api/v1/prs/${number}/merge`, {
      method: "POST",
      body: JSON.stringify({ squash }),
    });
  },
  listWorkspaces(): Promise<WorkspaceRecord[]> {
    return request<WorkspaceRecord[]>("/api/v1/workspaces");
  },
  listSnapshots(): Promise<SnapshotRecord[]> {
    return request<SnapshotRecord[]>("/api/v1/snapshots");
  },
};
