import { createPinia } from "pinia";
import { createApp } from "vue";

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
app.mount("#app");
