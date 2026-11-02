/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** The Neon Auth host after the endpoint id, e.g. `neonauth.c-7.eu-central-1.aws.neon.tech`. */
  readonly VITE_NEON_AUTH_HOST_SUFFIX?: string;
  /** The database in the Auth URL's path, e.g. `neondb`. */
  readonly VITE_NEON_AUTH_DB?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
