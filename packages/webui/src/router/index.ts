import { createRouter, createWebHashHistory } from "vue-router";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "prs", component: () => import("@/views/PullRequestsView") },
    { path: "/prs/:number", name: "pr-detail", component: () => import("@/views/PullRequestDetailView") },
    { path: "/workspaces", name: "workspaces", component: () => import("@/views/WorkspacesView") },
    { path: "/snapshots", name: "snapshots", component: () => import("@/views/SnapshotsView") },
  ],
});

export { router };
