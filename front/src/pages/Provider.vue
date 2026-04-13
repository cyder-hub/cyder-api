<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
            {{ $t("providerPage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("providerPage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto">
          <Button
            @click="router.push('/provider/new')"
            variant="outline"
            class="w-full sm:w-auto"
          >
            <Plus class="h-4 w-4 mr-1.5" />
            {{ $t("providerPage.addProvider") }}
          </Button>
        </div>
      </div>

      <div
        v-if="isLoading"
        class="flex items-center justify-center py-16 border border-gray-200 rounded-lg bg-white"
      >
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
        <span class="text-sm text-gray-500">{{ $t("providerPage.loading") }}</span>
      </div>

      <div
        v-else-if="error"
        class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
      >
        <AlertCircle class="h-10 w-10 stroke-1 text-red-400 mb-4" />
        <span class="text-sm font-medium text-red-500">{{
          $t("providerPage.error", { error: error })
        }}</span>
      </div>

      <div
        v-else-if="store.providers.length === 0"
        class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
      >
        <Inbox class="h-10 w-10 stroke-1 text-gray-400 mb-4" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("providerPage.noData")
        }}</span>
      </div>

      <div
        v-else
        class="grid grid-cols-1 gap-4 sm:gap-5 md:grid-cols-2 xl:grid-cols-3"
      >
        <div
          v-for="item in store.providers"
          :key="item.provider.id"
          class="flex h-full flex-col rounded-xl border border-gray-200 bg-white transition-colors hover:border-gray-300"
        >
          <div
            class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 sm:px-5"
          >
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <h3 class="min-w-0 text-base font-semibold text-gray-900">
                  {{ item.provider.name }}
                </h3>
                <Badge
                  variant="secondary"
                  class="max-w-full font-mono text-[10px] px-1.5 py-0"
                  >{{ item.provider.provider_type }}</Badge
                >
              </div>
              <p
                class="mt-1 truncate text-xs font-mono text-gray-400"
                :title="item.provider.provider_key"
              >
                {{ item.provider.provider_key }}
              </p>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                {{ $t("providerPage.table.apiKeys") }}: {{ item.provider_keys.length }}
              </Badge>
              <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                {{ $t("providerPage.table.models") }}: {{ item.models.length }}
              </Badge>
              <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                {{ $t("providerPage.table.useProxy") }}:
                {{ item.provider.use_proxy ? $t("common.yes") : $t("common.no") }}
              </Badge>
            </div>
          </div>

          <div class="flex-grow space-y-4 px-4 py-4 sm:px-5">
            <div class="grid grid-cols-1 gap-2 text-xs sm:gap-3">
              <div
                class="flex items-center justify-between rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5 text-gray-500"
              >
                <span class="flex items-center">
                  <Key class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
                  {{ $t("providerPage.table.apiKeys") }}
                </span>
                <span class="text-gray-900 font-medium">{{
                  item.provider_keys.length
                }}</span>
              </div>
              <div
                class="flex items-center justify-between rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5 text-gray-500"
              >
                <span class="flex items-center">
                  <Network class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
                  {{ $t("providerPage.table.useProxy") }}
                </span>
                <div class="flex items-center text-gray-700 font-medium">
                  <CheckCircle2
                    v-if="item.provider.use_proxy"
                    class="w-3.5 h-3.5 mr-1 text-gray-600"
                  />
                  <XCircle v-else class="w-3.5 h-3.5 mr-1 text-gray-300" />
                  {{
                    item.provider.use_proxy ? $t("common.yes") : $t("common.no")
                  }}
                </div>
              </div>
            </div>

            <div class="space-y-2.5">
              <span class="text-gray-500 flex items-center text-xs">
                <Box class="w-3.5 h-3.5 mr-1.5 text-gray-400" />
                {{ $t("providerPage.table.models") }} ({{ item.models.length }})
              </span>
              <div class="flex flex-wrap gap-1.5">
                <Badge
                  v-for="modelDetail in item.models.slice(0, 6)"
                  :key="modelDetail.model.id"
                  variant="secondary"
                  class="max-w-full truncate font-mono font-normal text-[11px] px-2 py-0.5 hover:bg-gray-200 hover:text-gray-900 cursor-pointer text-gray-500 transition-colors"
                  :title="
                    $t('providerPage.editModel', {
                      model_name: modelDetail.model.model_name,
                    })
                  "
                  @click="router.push(`/model/edit/${modelDetail.model.id}`)"
                >
                  {{ modelDetail.model.model_name }}
                </Badge>
                <Badge
                  v-if="item.models.length > 6"
                  variant="outline"
                  class="text-[11px] px-2 py-0.5 text-gray-400 bg-gray-50 border-gray-100"
                >
                  +{{ item.models.length - 6 }}
                </Badge>
              </div>
            </div>
          </div>

          <div
            class="flex flex-col gap-2 border-t border-gray-100 px-4 py-3 sm:flex-row sm:justify-end sm:px-5"
          >
            <Button
              variant="ghost"
              size="sm"
              class="w-full justify-center sm:w-auto"
              @click="router.push(`/provider/edit/${item.provider.id}`)"
            >
              <Pencil class="w-3.5 h-3.5 mr-1.5 text-gray-500" />
              {{ $t("common.edit") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full justify-center text-gray-400 hover:text-red-600 hover:bg-red-50 sm:w-auto"
              @click="handleDeleteProvider(item.provider)"
            >
              <Trash2 class="w-3.5 h-3.5 mr-1.5" />
              {{ $t("common.delete") }}
            </Button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { useProviderStore } from "@/store/providerStore";
import type { ProviderBase } from "@/store/types";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { confirm } from "@/lib/confirmController";
import { Api } from "@/services/request";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Plus,
  Loader2,
  AlertCircle,
  Inbox,
  Key,
  Network,
  Box,
  CheckCircle2,
  XCircle,
  Pencil,
  Trash2,
} from "lucide-vue-next";

const { t: $t } = useI18n();
const router = useRouter();
const store = useProviderStore();

const isLoading = ref(true);
const error = ref<string | null>(null);

const loadData = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    await store.fetchProviders();
  } catch (err: unknown) {
    error.value = normalizeError(
      err,
      $t("common.unknownError", "Unknown Error"),
    ).message;
  } finally {
    isLoading.value = false;
  }
};

onMounted(() => {
  loadData();
});

const handleDeleteProvider = async (provider: ProviderBase) => {
  await confirm($t("providerPage.confirmDelete", { name: provider.name }))
  try {
    await Api.deleteProvider(provider.id);
    toastController.success($t("deleteSuccess", "Deleted successfully"));
    await loadData();
  } catch (err: any) {
    console.error("Failed to delete provider:", err);
    const errorMessage = err.message || $t("common.unknownError", "Unknown Error");
    toastController.error($t("providerPage.deleteFailed", { error: errorMessage }));
  } 
};
</script>
