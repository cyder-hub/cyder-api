<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { RouterLink, RouterView, useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import LanguageSwitcher from "@/components/LanguageSwitcher.vue";
import { navItems, navSectionOrder, type NavSection } from "@/router/nav-items";
import { logout as logoutSession } from "@/services/auth";
import {
  LogOut,
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  X,
} from "lucide-vue-next";
import { Drawer, DrawerContent } from "@/components/ui/drawer";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

const isCollapsed = ref(false);
const isMobileNavOpen = ref(false);

type ManagerRouteMeta = {
  titleKey?: string;
  navKey?: string;
  parentNavKey?: string;
  navGroup?: NavSection;
};

const currentRouteMeta = computed(() => route.meta as ManagerRouteMeta);

const toggleSidebar = () => {
  isCollapsed.value = !isCollapsed.value;
};

const openMobileNav = () => {
  isMobileNavOpen.value = true;
};

const closeMobileNav = () => {
  isMobileNavOpen.value = false;
};

const handleLogout = async () => {
  await logoutSession();
  closeMobileNav();
  router.replace({ name: "Login" });
};

const translateNavItem = (item: { text?: string; i18nKey?: string }) => {
  if (item.i18nKey) {
    return t(item.i18nKey);
  }
  return item.text || "";
};

const activeNavKey = computed(
  () => currentRouteMeta.value.parentNavKey || currentRouteMeta.value.navKey || "",
);

const isLinkActive = (item: { path: string; navKey: string }) => {
  if (activeNavKey.value) {
    return item.navKey === activeNavKey.value;
  }
  return route.path === item.path || route.path.startsWith(`${item.path}/`);
};

const groupedNavItems = computed(() =>
  navSectionOrder
    .map((section) => ({
      section,
      items: navItems.filter((item) => item.section === section),
    }))
    .filter((group) => group.items.length > 0),
);

const currentPageTitle = computed(() => {
  if (currentRouteMeta.value.titleKey) {
    return t(currentRouteMeta.value.titleKey);
  }

  const matchedNavItem = activeNavKey.value
    ? navItems.find((item) => item.navKey === activeNavKey.value)
    : [...navItems]
        .sort((a, b) => b.path.length - a.path.length)
        .find((item) => isLinkActive(item));
  if (matchedNavItem) {
    return translateNavItem(matchedNavItem);
  }

  return t("app.header");
});

const documentTitle = computed(() => {
  const pageTitle = currentPageTitle.value;
  const appTitle = t("app.header");
  return pageTitle && pageTitle !== appTitle ? `${pageTitle} - ${appTitle}` : appTitle;
});

watch(
  () => route.fullPath,
  () => {
    closeMobileNav();
  },
);

watch(
  documentTitle,
  (title) => {
    document.title = title;
  },
  { immediate: true },
);
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
                  'bg-blue-50 text-blue-700': isLinkActive(item),
                  'text-gray-600 hover:bg-gray-100 hover:text-gray-900':
                    !isLinkActive(item),
                  'justify-center px-0': isCollapsed,
                }"
                :title="isCollapsed ? translateNavItem(item) : undefined"
              >
                <span
                  class="flex flex-shrink-0 items-center justify-center"
                  :class="
                    isLinkActive(item)
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

      <div class="px-2 pb-2">
        <button
          type="button"
          class="group flex w-full items-center rounded-md px-3 py-2 text-sm font-medium text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-900"
          :class="{ 'justify-center px-0': isCollapsed }"
          :title="isCollapsed ? t('app.logout') : undefined"
          :aria-label="t('app.logout')"
          @click="handleLogout"
        >
          <LogOut class="h-4 w-4 flex-shrink-0 text-gray-400 group-hover:text-gray-600" />
          <span
            v-if="!isCollapsed"
            class="ml-2.5 overflow-hidden whitespace-nowrap"
          >
            {{ t("app.logout") }}
          </span>
        </button>
      </div>

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

      <Drawer v-model:open="isMobileNavOpen" direction="left">
        <DrawerContent
          class="md:hidden flex flex-col h-full w-[min(20rem,calc(100vw-2rem))] sm:w-[min(20rem,calc(100vw-2rem))] md:w-[min(20rem,calc(100vw-2rem))] lg:w-[min(20rem,calc(100vw-2rem))] xl:w-[min(20rem,calc(100vw-2rem))] max-w-full rounded-none p-0 outline-none"
        >
          <div class="flex items-center gap-3 border-b border-gray-100 px-4 py-4">
            <div class="w-8 h-8 rounded-lg bg-gray-900 flex items-center justify-center flex-shrink-0">
              <span class="text-white font-bold text-sm leading-none tracking-wider">C</span>
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
                      isLinkActive(item)
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

          <div class="space-y-2 border-t border-gray-100 p-3">
            <button
              type="button"
              class="flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-sm font-medium text-gray-600 transition-colors hover:bg-gray-100 hover:text-gray-900"
              :aria-label="t('app.logout')"
              @click="handleLogout"
            >
              <LogOut class="h-4 w-4 flex-shrink-0 text-gray-400" />
              <span class="truncate">{{ t("app.logout") }}</span>
            </button>
            <LanguageSwitcher class="w-full" />
          </div>
        </DrawerContent>
      </Drawer>

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
