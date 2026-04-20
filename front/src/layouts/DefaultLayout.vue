<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import LanguageSwitcher from "@/components/LanguageSwitcher.vue";
import { navItems } from "@/lib/nav-items";
import {
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  X,
} from "lucide-vue-next";

const { t } = useI18n();
const route = useRoute();

const isCollapsed = ref(false);
const isMobileNavOpen = ref(false);

const toggleSidebar = () => {
  isCollapsed.value = !isCollapsed.value;
};

const openMobileNav = () => {
  isMobileNavOpen.value = true;
};

const closeMobileNav = () => {
  isMobileNavOpen.value = false;
};

const translateNavItem = (item: { text?: string; i18nKey?: string }) => {
  if (item.i18nKey) {
    return t(item.i18nKey);
  }
  return item.text || "";
};

const isLinkActive = (itemPath: string) => {
  return route.path === itemPath || route.path.startsWith(`${itemPath}/`);
};

const sectionOrder = ["start", "overview", "core", "advanced"] as const;

const groupedNavItems = computed(() =>
  sectionOrder
    .map((section) => ({
      section,
      items: navItems.filter((item) => item.section === section),
    }))
    .filter((group) => group.items.length > 0),
);

const currentPageTitle = computed(() => {
  const matchedNavItem = [...navItems]
    .sort((a, b) => b.path.length - a.path.length)
    .find((item) => isLinkActive(item.path));

  if (matchedNavItem) {
    return translateNavItem(matchedNavItem);
  }

  if (route.name === "ProviderNew") {
    return t("providerEditPage.titleAdd");
  }

  if (route.name === "ProviderEdit") {
    return t("providerEditPage.titleEdit");
  }

  if (route.name === "ModelEdit") {
    return t("modelEditPage.title");
  }

  return t("app.header");
});

watch(
  () => route.fullPath,
  () => {
    closeMobileNav();
  },
);

watch(isMobileNavOpen, (open) => {
  document.body.style.overflow = open ? "hidden" : "";
});
</script>

<template>
  <div class="flex h-dvh overflow-hidden bg-gray-50 font-sans">
    <aside
      class="hidden md:flex h-full overflow-hidden bg-white text-gray-700 flex-col flex-shrink-0 border-r border-gray-200 transition-all duration-300 ease-in-out z-20"
      :class="isCollapsed ? 'w-16' : 'w-56'"
    >
      <div
        class="flex items-center h-16 px-4 border-b border-gray-100 flex-shrink-0"
        :class="isCollapsed ? 'justify-center' : 'gap-3'"
      >
        <div
          class="w-8 h-8 rounded-lg bg-gray-900 flex items-center justify-center flex-shrink-0"
        >
          <span class="text-white font-bold text-sm leading-none tracking-wider">
            C
          </span>
        </div>
        <span
          v-if="!isCollapsed"
          class="text-base font-semibold text-gray-900 whitespace-nowrap overflow-hidden flex-1 tracking-tight"
        >
          {{ t("app.header") }}
        </span>
        <button
          v-if="!isCollapsed"
          @click="toggleSidebar"
          class="p-1 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 transition-colors focus:outline-none"
          :aria-label="t('app.toggleSidebar')"
        >
          <PanelLeftClose class="h-4 w-4" />
        </button>
      </div>

      <nav class="flex-grow overflow-y-auto overflow-x-hidden px-3 py-4">
        <div v-for="group in groupedNavItems" :key="group.section" class="mb-4 last:mb-0">
          <p
            v-if="!isCollapsed"
            class="px-3 pb-2 text-[11px] font-medium uppercase tracking-wider text-gray-400"
          >
            {{ t(`sidebar.sections.${group.section}`) }}
          </p>
          <ul class="list-none space-y-1">
            <li v-for="item in group.items" :key="item.path">
              <RouterLink
                :to="item.path"
                class="group flex items-center rounded-md py-2 px-3 text-sm font-medium transition-colors duration-200"
                :class="{
                  'bg-blue-50 text-blue-700': isLinkActive(item.path),
                  'text-gray-600 hover:bg-gray-100 hover:text-gray-900':
                    !isLinkActive(item.path),
                  'justify-center px-0': isCollapsed,
                }"
                :title="isCollapsed ? translateNavItem(item) : undefined"
              >
                <span
                  class="flex flex-shrink-0 items-center justify-center"
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
                  class="ml-2.5 overflow-hidden whitespace-nowrap"
                >
                  {{ translateNavItem(item) }}
                </span>
              </RouterLink>
            </li>
          </ul>
        </div>
      </nav>

      <button
        v-if="isCollapsed"
        @click="toggleSidebar"
        class="mx-auto mb-2 p-1.5 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 transition-colors focus:outline-none"
        :aria-label="t('app.toggleSidebar')"
      >
        <PanelLeftOpen class="h-4 w-4" />
      </button>

      <LanguageSwitcher :is-collapsed="isCollapsed" />
    </aside>

    <div class="min-w-0 flex-1 flex h-full flex-col overflow-hidden">
      <header
        class="md:hidden sticky top-0 z-30 flex h-14 items-center gap-3 border-b border-gray-200 bg-white/95 px-4 backdrop-blur supports-[backdrop-filter]:bg-white/80"
      >
        <button
          type="button"
          class="inline-flex h-9 w-9 items-center justify-center rounded-md border border-gray-200 text-gray-600 transition-colors hover:bg-gray-100 hover:text-gray-900"
          :aria-label="t('app.toggleSidebar')"
          @click="openMobileNav"
        >
          <Menu class="h-4 w-4" />
        </button>
        <div class="min-w-0 flex-1">
          <div class="truncate text-sm font-semibold text-gray-900">
            {{ currentPageTitle }}
          </div>
        </div>
        <LanguageSwitcher compact />
      </header>

      <Transition
        enter-active-class="transition-opacity duration-200"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition-opacity duration-200"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div
          v-if="isMobileNavOpen"
          class="md:hidden fixed inset-0 z-40 bg-gray-950/35"
          @click="closeMobileNav"
        />
      </Transition>

      <Transition
        enter-active-class="transition duration-300 ease-out"
        enter-from-class="-translate-x-full"
        enter-to-class="translate-x-0"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="translate-x-0"
        leave-to-class="-translate-x-full"
      >
        <aside
          v-if="isMobileNavOpen"
          class="md:hidden fixed inset-y-0 left-0 z-50 flex w-[min(20rem,calc(100vw-2rem))] max-w-full flex-col border-r border-gray-200 bg-white shadow-xl"
        >
          <div class="flex items-center gap-3 border-b border-gray-100 px-4 py-4">
            <div
              class="w-8 h-8 rounded-lg bg-gray-900 flex items-center justify-center flex-shrink-0"
            >
              <span class="text-white font-bold text-sm leading-none tracking-wider">
                C
              </span>
            </div>
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm font-semibold text-gray-900">
                {{ t("app.header") }}
              </div>
              <div class="truncate text-xs text-gray-500">
                {{ currentPageTitle }}
              </div>
            </div>
            <button
              type="button"
              class="inline-flex h-9 w-9 items-center justify-center rounded-md text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-900"
              :aria-label="t('common.close')"
              @click="closeMobileNav"
            >
              <X class="h-4 w-4" />
            </button>
          </div>

          <nav class="flex-1 overflow-y-auto px-3 py-4">
            <div v-for="group in groupedNavItems" :key="group.section" class="mb-4 last:mb-0">
              <p class="px-3 pb-2 text-[11px] font-medium uppercase tracking-wider text-gray-400">
                {{ t(`sidebar.sections.${group.section}`) }}
              </p>
              <ul class="list-none space-y-1">
                <li v-for="item in group.items" :key="item.path">
                  <RouterLink
                    :to="item.path"
                    class="flex items-center gap-3 rounded-md px-3 py-2.5 text-sm font-medium transition-colors"
                    :class="
                      isLinkActive(item.path)
                        ? 'bg-blue-50 text-blue-700'
                        : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900'
                    "
                  >
                    <component :is="item.icon" class="h-4 w-4 flex-shrink-0" />
                    <span class="truncate">{{ translateNavItem(item) }}</span>
                  </RouterLink>
                </li>
              </ul>
            </div>
          </nav>

          <div class="border-t border-gray-100 p-3">
            <LanguageSwitcher class="w-full" />
          </div>
        </aside>
      </Transition>

      <main class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
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
