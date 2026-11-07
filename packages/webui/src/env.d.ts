/// <reference types="vite/client" />

declare const __APP_VERSION__: string;
declare module "*.scss" { const href: string; export default href }

// Vite raw imports (source-contract tests read component sources as text).
declare module "*?raw" {
  const source: string;
  export default source;
}
