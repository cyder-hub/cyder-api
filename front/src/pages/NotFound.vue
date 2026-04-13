<template>
  <div class="app-page flex min-h-[calc(100dvh-var(--app-mobile-topbar-height,0px))] items-center">
    <div class="app-page-shell w-full">
      <section
        class="mx-auto flex max-w-lg flex-col items-center rounded-2xl border border-gray-200 bg-white px-5 py-10 text-center sm:px-8 sm:py-12"
      >
        <p class="text-xs font-medium uppercase tracking-[0.28em] text-gray-400">
          404
        </p>
        <h1 class="mt-4 text-4xl font-semibold tracking-tight text-gray-900 sm:text-5xl">
          {{ $t("common.pageNotFound", "Page Not Found") }}
        </h1>
        <p class="mt-4 max-w-md text-sm leading-6 text-gray-500">
          {{
            $t(
              "common.pageNotFoundDescription",
              "The page you are looking for does not exist or has been moved.",
            )
          }}
        </p>
        <Button
          @click="$router.push('/')"
          variant="default"
          class="mt-6 w-full sm:w-auto"
        >
          {{ $t("common.goHome", "Go Home") }}
        </Button>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Button } from "@/components/ui/button";
</script>
