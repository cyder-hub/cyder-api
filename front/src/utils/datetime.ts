import { resolveIntlLocale } from "../i18n/locale.ts";

const dateTimeFormatOptions: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
};

export function formatTimestamp(
  ms: number | string | Date | undefined | null,
  locale?: string | null,
): string {
  if (!ms) return "";

  try {
    const date = ms instanceof Date ? ms : new Date(ms);
    if (Number.isNaN(date.getTime())) return "";

    return new Intl.DateTimeFormat(
      resolveIntlLocale(locale),
      dateTimeFormatOptions,
    ).format(date);
  } catch {
    return "";
  }
}
