<script setup lang="ts">
import { useToast } from "./use-toast";
import Toast from "./Toast.vue";
import ToastProvider from "./ToastProvider.vue";
import ToastViewport from "./ToastViewport.vue";
import ToastTitle from "./ToastTitle.vue";
import ToastDescription from "./ToastDescription.vue";
import ToastClose from "./ToastClose.vue";

const { toasts, dismiss } = useToast();
</script>

<template>
  <ToastProvider>
    <template v-for="toast in toasts" :key="toast.id">
      <Toast
        :open="toast.open"
        :duration="toast.duration"
        :variant="toast.variant"
        :class="toast.class"
        @update:open="(open: boolean) => !open && dismiss(toast.id)"
      >
        <div class="grid gap-1">
          <ToastTitle v-if="toast.title">
            {{ toast.title }}
          </ToastTitle>
          <ToastDescription v-if="toast.description">
            {{ toast.description }}
          </ToastDescription>
        </div>
        <ToastClose />
      </Toast>
    </template>
    <ToastViewport />
  </ToastProvider>
</template>
