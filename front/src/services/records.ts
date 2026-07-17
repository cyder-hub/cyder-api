import { request } from "./http";
import { buildRecordListQuery } from "./query";
import type {
  PaginatedResponse,
  RecordDetail,
  RecordListItem,
  RecordListParams,
} from "./types";

export function getRecordList(
  params: RecordListParams,
): Promise<PaginatedResponse<RecordListItem>> {
  const qs = buildRecordListQuery(params);
  return request.get(`/ai/manager/api/request_log/list${qs ? `?${qs}` : ""}`);
}

export function getRecordDetail(id: number | string): Promise<RecordDetail> {
  return request.get(`/ai/manager/api/request_log/${id}`);
}
