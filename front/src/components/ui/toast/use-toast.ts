import { computed, ref } from "vue";
import type { Component, VNode, ComputedRef } from "vue";
import type { ToastProps } from "./types";

const TOAST_REMOVE_DELAY = 5000;

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
  variant?: ToastProps["variant"];
  class?: string;
}

interface ToasterToast extends Toast {
  id: string;
  open: boolean;
}

const toastsState = ref<ToasterToast[]>([]);

function toast(toast: Toast) {
  const id = genId();
  const newToast: ToasterToast = {
    ...toast,
    id,
    open: true,
  };

  toastsState.value = [...toastsState.value, newToast];

  setTimeout(() => {
    dismiss(id);
  }, TOAST_REMOVE_DELAY);

  return {
    id: newToast.id,
    dismiss: () => dismiss(newToast.id),
  };
}

function dismiss(id: string) {
  toastsState.value = toastsState.value.filter((toast) => toast.id !== id);
}

function useToast() {
  return {
    toasts: computed(() => toastsState.value) as ComputedRef<ToasterToast[]>,
    toast,
    dismiss,
  };
}

export { useToast, toast };
