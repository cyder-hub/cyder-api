export type QueryParams = object;

export function buildFilteredQueryString(params: QueryParams = {}): string {
  const qs = new URLSearchParams();
  for (const [key, value] of Object.entries(params as Record<string, unknown>)) {
    if (value === undefined || value === null || value === "") {
      continue;
    }
    if (Array.isArray(value)) {
      if (value.length === 0) {
        continue;
      }
      for (const item of value) {
        if (item === undefined || item === null || item === "") {
          continue;
        }
        qs.append(key, String(item));
      }
      continue;
    }
    qs.set(key, String(value));
  }
  return qs.toString();
}

export const buildAlertListQuery = buildFilteredQueryString;
export const buildNotificationDeliveryListQuery = buildFilteredQueryString;
export const buildProviderRuntimeListQuery = buildFilteredQueryString;
export const buildRecordListQuery = buildFilteredQueryString;

export function buildProviderRuntimeSummaryQuery(window?: string): string {
  return buildFilteredQueryString({ window });
}

export function buildSystemConfigHistoryQuery(params: {
  limit?: number;
  offset?: number;
} = {}): string {
  return buildFilteredQueryString(params);
}
