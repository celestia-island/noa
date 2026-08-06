import { ServerConfigBar } from "@/components/ServerConfigBar";
import { useServerStore } from "@/stores/server";

export default function App() {
  const store = useServerStore();
  void store.refreshAll();

  return (
    <div class="noa-app">
      <header class="noa-header">
        <div class="brand">
          <strong>noa</strong>
          <span>AI-native VCS</span>
        </div>
        <nav class="noa-nav">
          <router-link to="/">Pull Requests</router-link>
          <router-link to="/workspaces">Workspaces</router-link>
          <router-link to="/snapshots">Snapshots</router-link>
        </nav>
        <ServerConfigBar />
      </header>
      <main class="noa-main">
        <router-view />
      </main>
      <style>{`
        .noa-app { min-height: 100vh; display: flex; flex-direction: column; }
        .noa-header { display: flex; align-items: center; gap: 24px; padding: 12px 24px; border-bottom: 1px solid var(--noa-border, #333); }
        .brand { display: flex; align-items: baseline; gap: 8px; }
        .brand strong { font-size: 20px; }
        .brand span { font-size: 12px; opacity: 0.6; }
        .noa-nav { display: flex; gap: 16px; flex: 1; }
        .noa-nav a { color: inherit; text-decoration: none; }
        .noa-nav a.router-link-active { font-weight: 600; }
        .noa-main { flex: 1; padding: 24px; }
      `}</style>
    </div>
  );
}
