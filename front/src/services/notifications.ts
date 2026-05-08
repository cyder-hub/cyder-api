import { request } from "./http";
import { buildNotificationDeliveryListQuery } from "./query";
import type {
  NotificationChannel,
  NotificationChannelCreatePayload,
  NotificationChannelUpdatePayload,
  NotificationDeliveryListParams,
  NotificationDeliveryListResponse,
  NotificationWebhookTestResult,
} from "./types";

export function getNotificationChannels(): Promise<NotificationChannel[]> {
  return request.get("/ai/manager/api/notifications/channels");
}

export function getNotificationChannel(id: number): Promise<NotificationChannel> {
  return request.get(`/ai/manager/api/notifications/channels/${id}`);
}

export function createNotificationChannel(
  payload: NotificationChannelCreatePayload,
): Promise<NotificationChannel> {
  return request.post("/ai/manager/api/notifications/channels", payload);
}

export function updateNotificationChannel(
  id: number,
  payload: NotificationChannelUpdatePayload,
): Promise<NotificationChannel> {
  return request.put(`/ai/manager/api/notifications/channels/${id}`, payload);
}

export function deleteNotificationChannel(id: number): Promise<NotificationChannel> {
  return request.delete(`/ai/manager/api/notifications/channels/${id}`);
}

export function testNotificationChannel(
  id: number,
): Promise<NotificationWebhookTestResult> {
  return request.post(`/ai/manager/api/notifications/channels/${id}/test`, {});
}

export function getNotificationDeliveries(
  params: NotificationDeliveryListParams = {},
): Promise<NotificationDeliveryListResponse> {
  const qs = buildNotificationDeliveryListQuery(params);
  return request.get(
    `/ai/manager/api/notifications/deliveries${qs ? `?${qs}` : ""}`,
  );
}
