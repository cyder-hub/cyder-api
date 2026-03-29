import { reactive } from "vue";
import type { Component, VNode } from "vue";

const TOAST_REMOVE_DELAY = 1000000;

let count = 0;

function genId() {
  count = (count + 1) % 100;
  return count.toString();
}

type StringOrVNode = string | VNode | (() => VNode);

interface Toast {
  id?: string;
  title?: StringOrVNode;
  description?: StringOrVNode;
  action?: Component;
  duration?: number;
  variant?: "default" | "destructive" | string;
  class?: string;
}

interface ToasterToast extends Toast {
  id: string;
}

const state = reactive<{
  toasts: ToasterToast[];
}>({
  toasts: [],
});

function toast(toast: Toast) {
  const id = genId();
  const newToast = { ...toast, id };
  state.toasts.push(newToast);

  setTimeout(() => {
    state.toasts = state.toasts.filter((t) => t.id !== id);
  }, TOAST_REMOVE_DELAY);

  return {
    id: newToast.id,
    dismiss: () => dismiss(newToast.id),
  };
}

function dismiss(id: string) {
  state.toasts = state.toasts.filter((t) => t.id !== id);
}

function useToast() {
  return {
    toasts: state.toasts,
    toast,
    dismiss,
  };
}

export { useToast, toast };
