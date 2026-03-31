<script setup lang="ts">
import { ref } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import LanguageSwitcher from "@/components/LanguageSwitcher.vue";
import { navItems } from "@/lib/nav-items";
import { PanelLeftClose, PanelLeftOpen } from "lucide-vue-next";

const { t } = useI18n();
const route = useRoute();
const isCollapsed = ref(false);

const toggleSidebar = () => {
  isCollapsed.value = !isCollapsed.value;
};

const translateNavItem = (item: { text?: string; i18nKey?: string }) => {
  if (item.i18nKey) {
    return t(item.i18nKey);
  }
  return item.text || "";
};

const isLinkActive = (itemPath: string) => {
  return route.path === itemPath;
};
</script>

<template>
  <div class="flex h-screen bg-gray-50 font-sans">
    <!-- Sidebar -->
    <aside
      class="bg-white text-gray-700 flex flex-col flex-shrink-0 border-r border-gray-200 transition-all duration-300 ease-in-out z-10"
      :class="isCollapsed ? 'w-16' : 'w-56'"
    >
      <!-- Logo / Header -->
      <div
        class="flex items-center h-16 px-4 border-b border-gray-100 flex-shrink-0"
        :class="isCollapsed ? 'justify-center' : 'gap-3'"
      >
        <div
          class="w-8 h-8 rounded-lg bg-gray-900 flex items-center justify-center flex-shrink-0"
        >
          <span class="text-white font-bold text-sm leading-none tracking-wider"
            >C</span
          >
        </div>
        <span
          v-if="!isCollapsed"
          class="text-base font-semibold text-gray-900 whitespace-nowrap overflow-hidden flex-1 tracking-tight"
        >
          {{ t("appHeader") }}
        </span>
        <button
          v-if="!isCollapsed"
          @click="toggleSidebar"
          class="p-1 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 transition-colors focus:outline-none"
          :aria-label="t('toggleSidebar')"
        >
          <PanelLeftClose class="h-4 w-4" />
        </button>
      </div>

      <!-- Nav -->
      <nav class="flex-grow overflow-y-auto overflow-x-hidden py-4 px-3">
        <ul class="space-y-1 list-none">
          <li v-for="item in navItems" :key="item.path">
            <RouterLink
              :to="item.path"
              class="flex items-center py-2 px-3 rounded-md text-sm font-medium transition-colors duration-200 group"
              :class="{
                'bg-blue-50 text-blue-700': isLinkActive(item.path),
                'text-gray-600 hover:bg-gray-100 hover:text-gray-900':
                  !isLinkActive(item.path),
                'justify-center px-0': isCollapsed,
              }"
              :title="isCollapsed ? translateNavItem(item) : undefined"
            >
              <span
                class="flex items-center justify-center flex-shrink-0"
                :class="
                  isLinkActive(item.path)
                    ? 'text-blue-700'
                    : 'text-gray-400 group-hover:text-gray-500'
                "
              >
                <component :is="item.icon" class="h-4 w-4" />
              </span>
              <span
                v-if="!isCollapsed"
                class="ml-2.5 whitespace-nowrap overflow-hidden"
              >
                {{ translateNavItem(item) }}
              </span>
            </RouterLink>
          </li>
        </ul>
      </nav>

      <!-- Expand button when collapsed -->
      <button
        v-if="isCollapsed"
        @click="toggleSidebar"
        class="mx-auto mb-2 p-1.5 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 transition-colors focus:outline-none"
        :aria-label="t('toggleSidebar')"
      >
        <PanelLeftOpen class="h-4 w-4" />
      </button>

      <!-- Language Switcher -->
      <LanguageSwitcher :is-collapsed="isCollapsed" />
    </aside>

    <!-- Main Content -->
    <div class="flex-grow flex flex-col overflow-hidden">
      <main class="flex-grow p-6 overflow-y-auto">
        <RouterView />
      </main>
    </div>
  </div>
</template>

<style scoped>
nav::-webkit-scrollbar {
  width: 3px;
}
nav::-webkit-scrollbar-track {
  background: transparent;
}
nav::-webkit-scrollbar-thumb {
  background-color: #e5e7eb;
  border-radius: 20px;
}
</style>
