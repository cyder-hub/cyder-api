import { resolveIntlLocale } from "../i18n/locale.ts";

export function formatNumberValue(
  value: number,
  options?: Intl.NumberFormatOptions,
  locale?: string | null,
): string {
  return new Intl.NumberFormat(resolveIntlLocale(locale), options).format(value);
}
