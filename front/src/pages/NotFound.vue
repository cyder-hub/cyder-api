<template>
  <div
    class="h-full flex flex-col items-center justify-center p-8 text-center space-y-6"
  >
    <h1 class="text-6xl font-bold text-gray-800 dark:text-gray-200">404</h1>
    <h2 class="text-2xl font-semibold text-gray-600 dark:text-gray-400">
      {{ $t("common.pageNotFound", "Page Not Found") }}
    </h2>
    <p class="text-gray-500 max-w-md">
      {{
        $t(
          "common.pageNotFoundDescription",
          "The page you are looking for does not exist or has been moved.",
        )
      }}
    </p>
    <Button @click="$router.push('/')" variant="default" class="mt-4">
      {{ $t("common.goHome", "Go Home") }}
    </Button>
  </div>
</template>

<script setup lang="ts">
import { Button } from "@/components/ui/button";
</script>
