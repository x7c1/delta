/**
 * Importing an `.svg` yields its URL. ui-kit compiles with `tsc` alone, which
 * has no notion of the bundler's asset pipeline, so declare the shape here.
 * Local to this package — the web app already gets the same declaration from
 * `vite/client`.
 */
declare module '*.svg' {
  const url: string;
  export default url;
}
