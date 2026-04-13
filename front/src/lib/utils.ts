import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatTimestamp(ms: number | undefined | null): string {
  if (!ms) return "";
  try {
    const date = new Date(ms);
    if (isNaN(date.getTime())) return "";
    const YYYY = date.getFullYear();
    const MM = String(date.getMonth() + 1).padStart(2, "0");
    const DD = String(date.getDate()).padStart(2, "0");
    const hh = String(date.getHours()).padStart(2, "0");
    const mm = String(date.getMinutes()).padStart(2, "0");
    const ss = String(date.getSeconds()).padStart(2, "0");
    return `${YYYY}-${MM}-${DD} ${hh}:${mm}:${ss}`;
  } catch (e) {
    return "";
  }
}

const DEFAULT_CURRENCY_MINOR_UNIT_DIGITS = 2;
const CURRENCY_MINOR_UNIT_DIGITS: Record<string, number> = {
  BHD: 3,
  CLP: 0,
  CNY: 2,
  DJF: 0,
  EUR: 2,
  GBP: 2,
  GNF: 0,
  ISK: 0,
  JOD: 3,
  JPY: 0,
  KMF: 0,
  KRW: 0,
  KWD: 3,
  OMR: 3,
  PYG: 0,
  RWF: 0,
  TND: 3,
  UGX: 0,
  USD: 2,
  VND: 0,
  VUV: 0,
  XAF: 0,
  XOF: 0,
  XPF: 0,
};

export function getCurrencyMinorUnitDigits(currency?: string | null): number {
  if (!currency) {
    return DEFAULT_CURRENCY_MINOR_UNIT_DIGITS;
  }

  return CURRENCY_MINOR_UNIT_DIGITS[currency.toUpperCase()] ??
    DEFAULT_CURRENCY_MINOR_UNIT_DIGITS;
}

function getMajorUnitScale(currency?: string | null): number {
  return 10 ** (getCurrencyMinorUnitDigits(currency) + 9);
}

function getPerMillionRateScale(currency?: string | null): number {
  return 10 ** (getCurrencyMinorUnitDigits(currency) + 3);
}

function formatScaledValue(value: number, scaleDigits: number): string {
  return Number.isInteger(value)
    ? String(value)
    : value.toFixed(scaleDigits).replace(/\.?0+$/, "");
}

function parseScaledInteger(
  value: string,
  scaleDigits: number,
  errorMessage: string,
): number | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const pattern = new RegExp(`^\\d+(\\.\\d{1,${scaleDigits}})?$`);
  if (!pattern.test(trimmed)) {
    throw new Error(errorMessage);
  }

  const [integerPart, decimalPart = ""] = trimmed.split(".");
  return Number.parseInt(integerPart, 10) * (10 ** scaleDigits) +
    Number.parseInt(decimalPart.padEnd(scaleDigits, "0"), 10);
}

export function nanosToMajorUnit(nanos: number, currency?: string | null): number {
  return nanos / getMajorUnitScale(currency);
}

export function majorUnitToNanos(
  value: string,
  currency?: string | null,
): number | null {
  return parseScaledInteger(
    value,
    getCurrencyMinorUnitDigits(currency) + 9,
    "invalid price",
  );
}

export function formatPriceInputFromNanos(
  nanos: number | null | undefined,
  currency?: string | null,
): string {
  if (nanos === null || nanos === undefined) {
    return "";
  }

  return formatScaledValue(
    nanosToMajorUnit(nanos, currency),
    getCurrencyMinorUnitDigits(currency) + 9,
  );
}

export type CostRateInputMode = "money" | "per_million_units";

export function parseCostRateInputToNanos(
  value: string,
  mode: CostRateInputMode,
  currency?: string | null,
): number | null {
  if (mode === "money") {
    return majorUnitToNanos(value, currency);
  }

  return parseScaledInteger(
    value,
    getCurrencyMinorUnitDigits(currency) + 3,
    "invalid rate",
  );
}

export function formatCostRateInputFromNanos(
  nanos: number | null | undefined,
  mode: CostRateInputMode,
  currency?: string | null,
): string {
  if (mode === "money") {
    return formatPriceInputFromNanos(nanos, currency);
  }

  if (nanos === null || nanos === undefined) {
    return "";
  }

  return formatScaledValue(
    nanos / getPerMillionRateScale(currency),
    getCurrencyMinorUnitDigits(currency) + 3,
  );
}

export function formatCostRateFromNanos(
  nanos: number | null | undefined,
  mode: CostRateInputMode,
  currency?: string | null,
  fallback = "-",
): string {
  if (mode === "money") {
    return formatPriceFromNanos(nanos, currency, fallback);
  }

  if (nanos === null || nanos === undefined) {
    return fallback;
  }

  const value = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: getCurrencyMinorUnitDigits(currency) + 3,
  }).format(nanos / getPerMillionRateScale(currency));
  return currency ? `${currency} ${value} / 1M` : `${value} / 1M`;
}

export function formatPriceFromNanos(
  nanos: number | null | undefined,
  currency?: string | null,
  fallback = "-",
): string {
  if (nanos === null || nanos === undefined) {
    return fallback;
  }

  const value = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: getCurrencyMinorUnitDigits(currency) + 9,
  }).format(nanosToMajorUnit(nanos, currency));

  return currency ? `${currency} ${value}` : value;
}
