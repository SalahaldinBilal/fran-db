import type { clickOutside } from "../directives/clickOutside";

export * from "./agent";
export * from "./connection";
export * from "./db";
export * from "./errors";

declare module "solid-js" {
  namespace JSX {
    interface Directives {
      clickOutside?: ReturnType<Parameters<typeof clickOutside>[1]>;
    }
  }
}

export type Position = { x: number, y: number };
export type Size = { width: number, height: number };
