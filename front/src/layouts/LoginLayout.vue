<template>
  <div class="min-h-dvh bg-gray-50">
    <main
      class="min-h-dvh px-[var(--app-page-x)] py-[max(var(--app-page-y),env(safe-area-inset-top))] [padding-bottom:max(var(--app-page-y),env(safe-area-inset-bottom))]"
    >
      <router-view />
    </main>
  </div>
</template>

<script setup lang="ts"></script>
