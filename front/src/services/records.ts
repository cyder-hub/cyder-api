import { request } from "./http";
import { buildRecordListQuery } from "./query";
import type {
  PaginatedResponse,
  RecordArtifactResponse,
  RecordAttemptReplayExecuteParams,
  RecordAttemptReplayPreviewParams,
  RecordAttemptReplayPreviewResponse,
  RecordDiagnosticsRetentionParams,
  RecordDiagnosticsRetentionResponse,
  RecordDiagnosticsStorageInventoryParams,
  RecordDiagnosticsStorageInventoryResponse,
  RecordGatewayReplayExecuteParams,
  RecordGatewayReplayPreviewParams,
  RecordGatewayReplayPreviewResponse,
  RecordListItem,
  RecordDetail,
  RecordListParams,
  RecordReplayArtifact,
  RecordReplayRun,
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

export function getRecordArtifacts(
  id: number | string,
): Promise<RecordArtifactResponse> {
  return request.get(`/ai/manager/api/request_log/${id}/artifacts`);
}

export function previewAttemptReplay(
  id: number | string,
  attemptId: number | string,
  payload: RecordAttemptReplayPreviewParams = {},
): Promise<RecordAttemptReplayPreviewResponse> {
  return request.post(
    `/ai/manager/api/request_log/${id}/replay/attempt/${attemptId}/preview`,
    payload,
  );
}

export function executeAttemptReplay(
  id: number | string,
  attemptId: number | string,
  payload: RecordAttemptReplayExecuteParams,
): Promise<RecordReplayRun> {
  return request.post(
    `/ai/manager/api/request_log/${id}/replay/attempt/${attemptId}/execute`,
    payload,
  );
}

export function previewGatewayReplay(
  id: number | string,
  payload: RecordGatewayReplayPreviewParams = {},
): Promise<RecordGatewayReplayPreviewResponse> {
  return request.post(
    `/ai/manager/api/request_log/${id}/replay/gateway/preview`,
    payload,
  );
}

export function executeGatewayReplay(
  id: number | string,
  payload: RecordGatewayReplayExecuteParams,
): Promise<RecordReplayRun> {
  return request.post(
    `/ai/manager/api/request_log/${id}/replay/gateway/execute`,
    payload,
  );
}

export function getRecordReplayRuns(
  id: number | string,
): Promise<RecordReplayRun[]> {
  return request.get(`/ai/manager/api/request_log/${id}/replay`);
}

export function getRecordReplayRun(
  id: number | string,
  replayRunId: number | string,
): Promise<RecordReplayRun> {
  return request.get(`/ai/manager/api/request_log/${id}/replay/${replayRunId}`);
}

export function getRecordReplayArtifacts(
  id: number | string,
  replayRunId: number | string,
): Promise<RecordReplayArtifact> {
  return request.get(
    `/ai/manager/api/request_log/${id}/replay/${replayRunId}/artifacts`,
  );
}

export function getRecordContent(id: number | string): Promise<ArrayBuffer> {
  return request.get(`/ai/manager/api/request_log/${id}/content`, {
    headers: { "Content-Type": "application/msgpack" },
    responseType: "arraybuffer",
  });
}

export function previewRecordRetention(
  payload: RecordDiagnosticsRetentionParams,
): Promise<RecordDiagnosticsRetentionResponse> {
  return request.post("/ai/manager/api/request_log/retention/preview", payload);
}

export function executeRecordRetention(
  payload: RecordDiagnosticsRetentionParams,
): Promise<RecordDiagnosticsRetentionResponse> {
  return request.post("/ai/manager/api/request_log/retention/execute", payload);
}

export function previewRecordStorageInventory(
  payload: RecordDiagnosticsStorageInventoryParams,
): Promise<RecordDiagnosticsStorageInventoryResponse> {
  return request.post(
    "/ai/manager/api/request_log/storage_inventory/preview",
    payload,
  );
}
