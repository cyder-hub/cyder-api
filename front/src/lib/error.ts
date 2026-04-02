export const normalizeError = (
  error: unknown,
  fallback = "Unknown Error",
): Error => {
  if (error instanceof Error) {
    return error;
  }

  if (typeof error === "string" && error.trim()) {
    return new Error(error);
  }

  return new Error(fallback);
};
