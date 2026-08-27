import { HCard, HEmptyState, HSpinner, HTable } from "@celestia-island/hikari";

import { useServerStore } from "@/stores/server";

export default function WorkspacesView() {
  const store = useServerStore();

  const rows = store.workspaces.map((w) => ({
    name: w.name,
    head: w.head,
    base: w.base,
    last_seq: String(w.last_seq),
    updated_at: new Date(w.updated_at * 1000).toISOString(),
  }));

  return (
    <div class="workspaces-view">
      <h1>Workspaces</h1>
      {store.loading ? (
        <HSpinner center />
      ) : rows.length === 0 ? (
        <HEmptyState title="No workspaces" description="Workspaces appear once agents start working on the server." />
      ) : (
        <HCard>
          <HTable
  columns={[
    { key: "name", title: "Name" },
    { key: "head", title: "Head" },
    { key: "base", title: "Base" },
    { key: "last_seq", title: "Last seq" },
    { key: "updated_at", title: "Updated" },
  ]}
  rows={rows}
/>
        </HCard>
      )}
      <style>{`.workspaces-view { display: flex; flex-direction: column; gap: 16px; }`}</style>
    </div>
  );
}
