import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";

import WorkspacesView from "./WorkspacesView";
import SnapshotsView from "./SnapshotsView";
import PullRequestsView from "./PullRequestsView";
import { useServerStore } from "@/stores/server";

/**
 * The 2026-10-18 hikari jump guard (0.41.3 → 0.55.188): noa webui has no
 * other test surface, so this smoke mounts every roster view against the
 * real hikari bundle and pins the interaction contract the jump bought —
 *
 * - the tables render through HkTable with the copy layer armed
 *   (`data-copy-cells`) and the default-rendered cells advertise the
 *   click-to-copy hint;
 * - the slot-rendered PR title cells keep their own link semantics;
 * - empty stores paint the standard empty state, not a crash.
 */

describe("roster views on hikari 0.55.x", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("WorkspacesView: copy layer on, hint on default cells", () => {
    const store = useServerStore();
    store.workspaces = [
      {
        name: "celestia",
        head: "a1b2c3d",
        base: "main",
        last_seq: 42,
        updated_at: 1760000000,
      } as never,
    ];
    const w = mount(WorkspacesView, { global: { plugins: [] } });
    expect(w.find("table.hk-table[data-copy-cells]").exists()).toBe(true);
    const tds = w.findAll("tbody td.hk-table-cell");
    expect(tds.length).toBe(5);
    expect(tds[0]!.attributes("title")).toBe("Click to copy");
    expect(w.text()).toContain("celestia");
  });

  it("SnapshotsView: renders rows through the standard table", () => {
    const store = useServerStore();
    store.snapshots = [
      {
        id: "snap-1",
        workspace: "celestia",
        message: "first",
        author: "demiurge",
        timestamp: 1760000000,
      } as never,
    ];
    const w = mount(SnapshotsView);
    expect(w.find("table.hk-table[data-copy-cells]").exists()).toBe(true);
    expect(w.text()).toContain("snap-1");
  });

  it("PullRequestsView: slot cells keep their own affordances beside the copy layer", async () => {
    const store = useServerStore();
    store.prs = [
      {
        number: 7,
        title: "Support the jump",
        author: "demiurge",
        updated_at: 1760000000,
        state: "open",
      } as never,
    ];
    const w = mount(PullRequestsView);
    expect(w.find("table.hk-table[data-copy-cells]").exists()).toBe(true);
    // The title cell renders the PR link (its own semantics), not a copy hint.
    const titleTd = w
      .findAll("tbody td.hk-table-cell")
      .find((td) => td.text().includes("Support the jump"))!;
    expect(titleTd.attributes("title")).toBeUndefined();
    expect(w.text()).toContain("Support the jump");
  });

  it("empty stores paint the standard empty state", () => {
    const w = mount(WorkspacesView);
    expect(w.find(".hk-empty-state").exists()).toBe(true);
  });
});
