import { computed, ref, watchEffect } from "vue";
import { useRoute, useRouter } from "vue-router";

import { HButton, HCard, HSpinner } from "@celestia-island/hikari";

import { api } from "@/api/client";
import type { PrRecord } from "@/api/client";

export default function PullRequestDetailView() {
  const route = useRoute();
  const router = useRouter();
  const number = computed(() => Number(route.params["number"]));

  const pr = ref<PrRecord | null>(null);
  const error = ref<string | null>(null);
  const merging = ref(false);

  watchEffect(() => {
    const n = number.value;
    if (!Number.isFinite(n)) return;
    pr.value = null;
    error.value = null;
    api
      .getPr(n)
      .then((p) => (pr.value = p))
      .catch((e) => (error.value = e instanceof Error ? e.message : String(e)));
  });

  const merge = async (squash: boolean) => {
    if (!pr.value) return;
    merging.value = true;
    error.value = null;
    try {
      pr.value = await api.mergePr(pr.value.number, squash);
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      merging.value = false;
    }
  };

  return (
    <div class="pr-detail">
      <HButton size="sm" variant="ghost" onClick={() => router.push("/")}>
        ← Back
      </HButton>
      {error.value && <span class="err">{error.value}</span>}
      {!pr.value && !error.value && <HSpinner />}
      {pr.value && (
        <HCard>
          <h1>
            #{pr.value.number} {pr.value.title}
          </h1>
          <dl>
            <dt>State</dt>
            <dd>{pr.value.state}</dd>
            <dt>Base</dt>
            <dd>{pr.value.base}</dd>
            <dt>Head</dt>
            <dd>{pr.value.head}</dd>
            <dt>Author</dt>
            <dd>{pr.value.author}</dd>
            <dt>Created</dt>
            <dd>{new Date(pr.value.created_at * 1000).toISOString()}</dd>
          </dl>
          {pr.value.metadata && (
            <pre class="meta">{JSON.stringify(pr.value.metadata, null, 2)}</pre>
          )}
          <p class="body">{pr.value.body || "(no body)"}</p>
          {pr.value.state === "open" && (
            <div class="actions">
              <HButton disabled={merging.value} onClick={() => void merge(true)}>
                {merging.value ? "Merging..." : "Merge (squash)"}
              </HButton>
            </div>
          )}
        </HCard>
      )}
      <style>{`
        .pr-detail { display: flex; flex-direction: column; gap: 16px; max-width: 720px; }
        .err { color: #f33; }
        dl { display: grid; grid-template-columns: 120px 1fr; gap: 8px; }
        dt { font-weight: 600; }
        pre.meta { background: #111; padding: 12px; border-radius: 8px; overflow: auto; }
        .body { white-space: pre-wrap; }
        .actions { display: flex; gap: 8px; }
      `}</style>
    </div>
  );
}
