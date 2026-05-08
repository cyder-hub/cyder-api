import { computed, ref, watch } from "vue";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostCatalogVersionDetail,
} from "../../../services/types/cost";

export interface CostCatalogsApiClient {
  getCostCatalogList: () => Promise<CostCatalogListItem[]>;
  getCostCatalogVersion: (id: number) => Promise<CostCatalogVersionDetail>;
}

export function resolveCostCatalogSelection(
  catalogs: CostCatalogListItem[],
  selectedCatalogId: number | null,
) {
  if (
    selectedCatalogId !== null &&
    catalogs.some((item) => item.catalog.id === selectedCatalogId)
  ) {
    return selectedCatalogId;
  }

  return catalogs[0]?.catalog.id ?? null;
}

export function resolveCostVersionSelection(
  catalogs: CostCatalogListItem[],
  catalogId: number | null,
  selectedVersionId: number | null,
) {
  const versions =
    catalogs.find((item) => item.catalog.id === catalogId)?.versions ?? [];
  if (versions.some((version) => version.id === selectedVersionId)) {
    return selectedVersionId;
  }

  return versions[0]?.id ?? null;
}

export function resolvePreferredCostVersionId(
  versions: CostCatalogVersion[],
  preferredVersionId: number | null,
  showArchivedVersions: boolean,
) {
  const visibleVersions = showArchivedVersions
    ? versions
    : versions.filter((version) => !version.is_archived);

  if (
    preferredVersionId !== null &&
    visibleVersions.some((version) => version.id === preferredVersionId)
  ) {
    return preferredVersionId;
  }

  return visibleVersions[0]?.id ?? null;
}

export function useCostCatalogs(api: CostCatalogsApiClient) {
  const catalogs = ref<CostCatalogListItem[]>([]);
  const versionDetail = ref<CostCatalogVersionDetail | null>(null);
  const selectedCatalogId = ref<number | null>(null);
  const selectedVersionId = ref<number | null>(null);
  const isLoadingCatalogs = ref(false);
  const isLoadingVersionDetail = ref(false);

  const selectedCatalog = computed(() =>
    catalogs.value.find((item) => item.catalog.id === selectedCatalogId.value) ??
    null,
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
      catalogs.value = await api.getCostCatalogList();
      selectedCatalogId.value = resolveCostCatalogSelection(
        catalogs.value,
        selectedCatalogId.value,
      );
      selectedVersionId.value = resolveCostVersionSelection(
        catalogs.value,
        selectedCatalogId.value,
        selectedVersionId.value,
      );
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
      versionDetail.value = await api.getCostCatalogVersion(versionId);
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
    selectedVersionId.value = resolveCostVersionSelection(
      catalogs.value,
      catalogId,
      selectedVersionId.value,
    );
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
}
