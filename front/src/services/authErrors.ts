export function authErrorCode(error: unknown): number | null {
  if (!error || typeof error !== "object" || !("response" in error)) {
    return null;
  }
  const response = (error as { response?: { data?: { code?: unknown } } }).response;
  return typeof response?.data?.code === "number" ? response.data.code : null;
}
