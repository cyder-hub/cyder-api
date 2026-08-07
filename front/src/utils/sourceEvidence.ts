export interface SourceIdentityEvidence {
  source_id: number | null;
  source_profile_type: string | null;
}

export function formatSourceIdentity(
  source: SourceIdentityEvidence,
  unselectedLabel: string,
): string {
  if (source.source_id == null) {
    return unselectedLabel;
  }

  const identity = `#${source.source_id}`;
  const profile = source.source_profile_type?.trim();
  return profile ? `${identity} · ${profile}` : identity;
}

export function formatSafeSourceEndpoint(
  endpoint: string | null | undefined,
  emptyLabel: string,
): string {
  if (!endpoint) return emptyLabel;

  try {
    const parsed = new URL(endpoint);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return emptyLabel;
    }
    return `${parsed.protocol}//${parsed.host}${parsed.pathname}`;
  } catch {
    return emptyLabel;
  }
}
