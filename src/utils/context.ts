// provide/inject 上下文（architecture.md §8：只用于 toast() 与 setBanner()）
import type { InjectionKey } from "vue";
import type { ToastTone } from "../types";

export type BannerTone = "error" | "demo";

export interface UiContext {
  toast: (message: string, tone?: ToastTone) => void;
  setBanner: (message: string | null, tone?: BannerTone) => void;
}

export const UI_CONTEXT_KEY: InjectionKey<UiContext> = Symbol("worktree-manager-ui");
