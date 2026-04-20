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
};

export const navItems: NavItem[] = [
  {
    path: "/dashboard",
    i18nKey: "sidebar.dashboard",
    icon: LayoutDashboard,
  },
  {
    path: "/record",
    i18nKey: "sidebar.record",
    icon: ClipboardList,
  },
  {
    path: "/provider",
    i18nKey: "sidebar.provider",
    icon: Server,
  },
  {
    path: "/provider/runtime",
    i18nKey: "sidebar.providerRuntime",
    icon: Activity,
  },
  {
    path: "/api_key",
    i18nKey: "sidebar.apiKey",
    icon: KeyRound,
  },
  {
    path: "/model_route",
    i18nKey: "sidebar.modelRoute",
    icon: ArrowRightLeft,
  },
  {
    path: "/custom_fields",
    i18nKey: "sidebar.customFields",
    icon: Type,
  },
  {
    path: "/cost",
    i18nKey: "sidebar.cost",
    icon: DollarSign,
  },
];
