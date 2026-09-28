// esbuild bundles `.hd` sources as text and `.css` as a stylesheet (build.ts).
declare module "*.hd" {
  const source: string;
  export default source;
}

declare module "*.css";
