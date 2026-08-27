import { HCard, HEmptyState, HSpinner, HTable } from "@celestia-island/hikari";

import { useServerStore } from "@/stores/server";

export default function SnapshotsView() {
  const store = useServerStore();

  const rows = [...store.snapshots]
    .sort((a, b) => b.timestamp - a.timestamp)
    .map((s) => ({
      id: s.id,
      workspace: s.workspace,
      message: s.message,
      author: s.author,
      timestamp: new Date(s.timestamp / 1000).toISOString(),
    }));

  return (
    <div class="snapshots-view">
      <h1>Snapshots</h1>
      {store.loading ? (
        <HSpinner center />
      ) : rows.length === 0 ? (
        <HEmptyState title="No snapshots" description="Snapshots are created when agents commit their work." />
      ) : (
        <HCard>
          <HTable
  columns={[
    { key: "id", title: "ID" },
    { key: "workspace", title: "Workspace" },
    { key: "message", title: "Message" },
    { key: "author", title: "Author" },
    { key: "timestamp", title: "Timestamp" },
  ]}
  rows={rows}
/>
        </HCard>
      )}
      <style>{`.snapshots-view { display: flex; flex-direction: column; gap: 16px; }`}</style>
    </div>
  );
}
