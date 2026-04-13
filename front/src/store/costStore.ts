import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { Api } from "@/services/request";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostCatalogVersionDetail,
} from "./types";

export const useCostStore = defineStore("cost", () => {
  const catalogs = ref<CostCatalogListItem[]>([]);
  const versionDetail = ref<CostCatalogVersionDetail | null>(null);
  const selectedCatalogId = ref<number | null>(null);
  const selectedVersionId = ref<number | null>(null);
  const isLoadingCatalogs = ref(false);
  const isLoadingVersionDetail = ref(false);

  const selectedCatalog = computed(() =>
    catalogs.value.find((item) => item.catalog.id === selectedCatalogId.value) ?? null,
  );

  const selectedCatalogVersions = computed<CostCatalogVersion[]>(() => {
    return selectedCatalog.value?.versions ?? [];
  });

  const selectedVersion = computed(() => {
    return (
      selectedCatalogVersions.value.find(
        (version) => version.id === selectedVersionId.value,
      ) ?? null
    );
  });

  async function fetchCatalogs() {
    isLoadingCatalogs.value = true;
    try {
      catalogs.value = await Api.getCostCatalogList();
      if (
        selectedCatalogId.value !== null &&
        !catalogs.value.some((item) => item.catalog.id === selectedCatalogId.value)
      ) {
        selectedCatalogId.value = null;
      }
      if (selectedCatalogId.value === null && catalogs.value.length > 0) {
        selectedCatalogId.value = catalogs.value[0].catalog.id;
      }
    } finally {
      isLoadingCatalogs.value = false;
    }
  }

  async function fetchVersionDetail(versionId: number | null) {
    if (versionId === null) {
      versionDetail.value = null;
      return;
    }
    isLoadingVersionDetail.value = true;
    try {
      versionDetail.value = await Api.getCostCatalogVersion(versionId);
    } finally {
      isLoadingVersionDetail.value = false;
    }
  }

  async function refreshCurrentVersionDetail() {
    await fetchVersionDetail(selectedVersionId.value);
  }

  function setSelectedCatalogId(catalogId: number | null) {
    selectedCatalogId.value = catalogId;
  }

  function setSelectedVersionId(versionId: number | null) {
    selectedVersionId.value = versionId;
  }

  watch(selectedCatalogId, (catalogId) => {
    const versions =
      catalogs.value.find((item) => item.catalog.id === catalogId)?.versions ?? [];
    if (!versions.some((version) => version.id === selectedVersionId.value)) {
      selectedVersionId.value = versions[0]?.id ?? null;
    }
  });

  watch(
    selectedVersionId,
    (versionId) => {
      void fetchVersionDetail(versionId);
    },
    { immediate: true },
  );

  return {
    catalogs,
    versionDetail,
    selectedCatalogId,
    selectedVersionId,
    selectedCatalog,
    selectedCatalogVersions,
    selectedVersion,
    isLoadingCatalogs,
    isLoadingVersionDetail,
    fetchCatalogs,
    fetchVersionDetail,
    refreshCurrentVersionDetail,
    setSelectedCatalogId,
    setSelectedVersionId,
  };
});
