import { VistalithClient } from "@vistalith/client";

export const vistalithdUrl =
  import.meta.env.VITE_VISTALITHD_URL ?? "http://127.0.0.1:7420";

export const client = new VistalithClient({ baseUrl: vistalithdUrl });
