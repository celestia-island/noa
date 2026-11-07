import { createPinia } from "pinia";
import { createApp } from "vue";

import { createErrorReporting } from "@celestia-island/hikari";

import App from "./App";
import { router } from "./router";
import { applyViewportPolicy } from "./mobileViewport";
import "./mobile-ux.scss";

// Mobile UX contract (hikari #325 sibling): normalize the viewport meta
// before first paint so phones never refuse pinch zoom; the tap-highlight
// reset ships via mobile-ux.scss.
applyViewportPolicy({ allowZoomOut: true });

const app = createApp(App);
app.use(createPinia());
app.use(router);
// Global error reporting (hikari createErrorReporting): any uncaught
// error — render crash, event-handler throw, uncaught window error /
// rejection — raises the unified full-viewport error landing instead of
// a blank pane. The UI is English-only, matching the context default.
app.use(createErrorReporting());
app.mount("#app");
