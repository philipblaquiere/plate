/// <reference types="vite/client" />

declare module "*.ncl?raw" {
  const source: string;
  export default source;
}

declare module "*.wasm?url" {
  const url: string;
  export default url;
}
