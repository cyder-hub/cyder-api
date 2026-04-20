import type { ModelSummaryItem } from "@/store/types";

const sortModelSummaries = (models: ModelSummaryItem[]) =>
  [...models].sort((left, right) => {
    return (
      left.provider_name.localeCompare(right.provider_name) ||
      left.model_name.localeCompare(right.model_name)
    );
  });

export const buildModelPageState = (models: ModelSummaryItem[], query: string) => {
  const normalizedQuery = query.trim().toLowerCase();
  const sortedItems = sortModelSummaries(models);
  const filteredItems = !normalizedQuery
    ? sortedItems
    : sortedItems.filter((item) => {
        const haystack = [
          item.provider_name,
          item.provider_key,
          item.model_name,
          item.real_model_name || "",
        ]
          .join(" ")
          .toLowerCase();
        return haystack.includes(normalizedQuery);
      });

  return {
    filteredItems,
    isPageEmpty: models.length === 0,
    isSearchEmpty: models.length > 0 && filteredItems.length === 0,
  };
};
