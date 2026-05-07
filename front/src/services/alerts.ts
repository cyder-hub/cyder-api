import { request } from "./http";
import { buildAlertListQuery } from "./query";
import type {
  AlertAckPayload,
  AlertEvent,
  AlertListParams,
  AlertListResponse,
  AlertSuppressPayload,
} from "./types";

export function getAlerts(
  params: AlertListParams = {},
): Promise<AlertListResponse> {
  const qs = buildAlertListQuery(params);
  return request.get(`/ai/manager/api/alerts/list${qs ? `?${qs}` : ""}`);
}

export function getAlert(id: number): Promise<AlertEvent> {
  return request.get(`/ai/manager/api/alerts/${id}`);
}

export function acknowledgeAlert(
  id: number,
  payload: AlertAckPayload,
): Promise<AlertEvent> {
  return request.post(`/ai/manager/api/alerts/${id}/ack`, payload);
}

export function suppressAlert(
  id: number,
  payload: AlertSuppressPayload,
): Promise<AlertEvent> {
  return request.post(`/ai/manager/api/alerts/${id}/suppress`, payload);
}

export function unsuppressAlert(id: number): Promise<AlertEvent> {
  return request.post(`/ai/manager/api/alerts/${id}/unsuppress`, {});
}

export function resolveAlert(id: number): Promise<AlertEvent> {
  return request.post(`/ai/manager/api/alerts/${id}/resolve`, {});
}
