import type { Component } from "vue";
import {
  LayoutDashboard,
  KeyRound,
  ArrowRightLeft,
  Type,
  DollarSign,
  ClipboardList,
  Server,
  Activity,
} from "lucide-vue-next";

type NavItem = {
  path: string;
  i18nKey: string;
  icon: Component;
  section: "start" | "overview" | "core" | "advanced";
};

export const navItems: NavItem[] = [
  {
    path: "/provider",
    i18nKey: "sidebar.provider",
    icon: Server,
    section: "start",
  },
  {
    path: "/dashboard",
    i18nKey: "sidebar.dashboard",
    icon: LayoutDashboard,
    section: "overview",
  },
  {
    path: "/record",
    i18nKey: "sidebar.record",
    icon: ClipboardList,
    section: "core",
  },
  {
    path: "/provider/runtime",
    i18nKey: "sidebar.providerRuntime",
    icon: Activity,
    section: "core",
  },
  {
    path: "/model",
    i18nKey: "sidebar.model",
    icon: ArrowRightLeft,
    section: "core",
  },
  {
    path: "/api_key",
    i18nKey: "sidebar.apiKey",
    icon: KeyRound,
    section: "advanced",
  },
  {
    path: "/model_route",
    i18nKey: "sidebar.modelRoute",
    icon: ArrowRightLeft,
    section: "advanced",
  },
  {
    path: "/custom_fields",
    i18nKey: "sidebar.customFields",
    icon: Type,
    section: "advanced",
  },
  {
    path: "/cost",
    i18nKey: "sidebar.cost",
    icon: DollarSign,
    section: "advanced",
  },
];
