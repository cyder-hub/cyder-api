<script setup lang="ts">
import GlobalConfirmDialog from "@/components/GlobalConfirmDialog.vue";
import Toaster from "@/components/ui/toast/Toaster.vue";
</script>

<template>
  <router-view />
  <Toaster />
  <GlobalConfirmDialog />
</template>

<style scoped></style>
