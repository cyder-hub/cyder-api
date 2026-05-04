import type { Component } from "vue";
import {
  LayoutDashboard,
  KeyRound,
  ArrowRightLeft,
  DollarSign,
  ClipboardList,
  Server,
  Activity,
  BellRing,
  Settings,
  Webhook,
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
    path: "/alerts",
    i18nKey: "sidebar.alerts",
    icon: BellRing,
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
    path: "/cost",
    i18nKey: "sidebar.cost",
    icon: DollarSign,
    section: "advanced",
  },
  {
    path: "/notifications",
    i18nKey: "sidebar.notifications",
    icon: Webhook,
    section: "advanced",
  },
  {
    path: "/system/config",
    i18nKey: "sidebar.systemConfig",
    icon: Settings,
    section: "advanced",
  },
];
