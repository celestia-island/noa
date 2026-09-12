import { ref } from "vue";

import { HButton, HInput } from "@celestia-island/hikari";

import { useServerStore } from "@/stores/server";

export function ServerConfigBar() {
  const store = useServerStore();
  const base = ref(store.baseUrl);
  const apiToken = ref(store.token);
  const open = ref(false);

  if (!open.value) {
    return (
      <div class="server-config">
        <span class={store.connected ? "dot ok" : "dot"} title={store.error || undefined} />
        <HButton size="sm" variant="ghost" onClick={() => (open.value = true)}>
          Server
        </HButton>
        <style>{`
          .server-config { display: flex; align-items: center; gap: 8px; }
          .dot { width: 8px; height: 8px; border-radius: 50%; background: #f00; display: inline-block; }
          .dot.ok { background: #0c6; }
        `}</style>
      </div>
    );
  }

  const save = () => {
    store.configure(base.value.trim(), apiToken.value.trim());
    void store.refreshAll();
    open.value = false;
  };

  return (
    <div class="server-config-form">
      <HInput v-model={base.value} placeholder="http://127.0.0.1:3000" label="Server URL" />
      <HInput v-model={apiToken.value} type="password" placeholder="NOA_API_TOKEN" label="API token" />
      <HButton onClick={save}>
        Connect
      </HButton>
      {store.error && <span class="err">{store.error}</span>}
      <style>{`
        /* Default (md) button height matches the sibling inputs' 40px box; the
           compact sm "Server" chip in the closed header state stays a deliberate
           header affordance. */
        .server-config-form { display: flex; align-items: flex-end; gap: var(--space-8, 8px); }
      `}</style>
    </div>
  );
}
