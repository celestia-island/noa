import { computed, ref } from "vue";
import { useRouter } from "vue-router";

import { HButton, HCard, HEmptyState, HInput, HSpinner, HTable } from "@celestia-island/hikari";

import { api } from "@/api/client";
import { useServerStore } from "@/stores/server";

export default function PullRequestsView() {
  const store = useServerStore();
  const router = useRouter();

  const showCreate = ref(false);
  const title = ref("");
  const body = ref("");
  const baseBranch = ref("master");
  const headBranch = ref("");
  const creating = ref(false);
  const createError = ref<string | null>(null);

  const rows = computed(() =>
    store.prs.map((p) => ({
      number: String(p.number),
      title: p.title,
      state: p.state,
      base: p.base,
      head: p.head,
      author: p.author,
    })),
  );

  const columns = [
  { key: "number", title: "#" },
  { key: "title", title: "Title" },
  { key: "state", title: "State" },
  { key: "base", title: "Base" },
  { key: "head", title: "Head" },
  { key: "author", title: "Author" },
];

  const createPr = async () => {
    creating.value = true;
    createError.value = null;
    try {
      await api.createPr({
        title: title.value.trim(),
        body: body.value.trim(),
        base: baseBranch.value.trim() || "master",
        head: headBranch.value.trim(),
        author: "noa-webui",
      });
      title.value = "";
      body.value = "";
      headBranch.value = "";
      showCreate.value = false;
      await store.refreshPrs();
    } catch (e) {
      createError.value = e instanceof Error ? e.message : String(e);
    } finally {
      creating.value = false;
    }
  };

  return (
    <div class="prs-view">
      <div class="row">
        <h1>Pull Requests</h1>
        <HButton onClick={() => (showCreate.value = !showCreate.value)}>
          {showCreate.value ? "Cancel" : "New PR"}
        </HButton>
      </div>

      {showCreate.value && (
        <HCard class="create-card">
          <HInput v-model={title.value} label="Title" placeholder="✨ Add feature." />
          <HInput v-model={baseBranch.value} label="Base branch" />
          <HInput v-model={headBranch.value} label="Head branch" placeholder="feat/..." />
          <HInput v-model={body.value} label="Body" placeholder="Description" />
          {createError.value && <span class="err">{createError.value}</span>}
          <HButton disabled={creating.value || !headBranch.value.trim()} onClick={() => void createPr()}>
            {creating.value ? "Creating..." : "Create PR"}
          </HButton>
        </HCard>
      )}

      {store.loading ? (
        <HSpinner center />
      ) : rows.value.length === 0 ? (
        <HEmptyState title="No pull requests" description="Connect to a noa-server and create the first PR." />
      ) : (
        <HTable
          columns={columns}
          rows={rows.value}
          v-slots={{
            "cell-title": ({ row }: { row: Record<string, unknown> }) => {
              // A malformed row without a number must not render the
              // literal "undefined" into the hash route.
              const num = row["number"];
              if (typeof num !== "string" && typeof num !== "number") {
                return <span>{String(row["title"] ?? "")}</span>;
              }
              return <a href={`#/prs/${num}`}>{String(row["title"] ?? "")}</a>;
            },
          }}
        />
      )}
      <style>{`
        .prs-view { display: flex; flex-direction: column; gap: 16px; }
        .row { display: flex; align-items: center; justify-content: space-between; }
        .create-card { display: flex; flex-direction: column; gap: 12px; max-width: 480px; }
        .err { color: #f33; font-size: 12px; }
      `}</style>
    </div>
  );
}
