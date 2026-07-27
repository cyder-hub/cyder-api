import type { Component } from "vue";
import {
  Activity,
  ArrowRightLeft,
  ClipboardList,
  DollarSign,
  KeyRound,
  LayoutDashboard,
  Server,
  ShieldCheck,
} from "lucide-vue-next";

type NavItem = {
  path: string;
  i18nKey: string;
  icon: Component;
  navKey: string;
  section: NavSection;
};

export type NavSection = "operations" | "traffic" | "resources" | "governance";

export const navSectionOrder: NavSection[] = [
  "operations",
  "traffic",
  "resources",
  "governance",
];

export const navItems: NavItem[] = [
  {
    path: "/dashboard",
    navKey: "dashboard",
    i18nKey: "sidebar.dashboard",
    icon: LayoutDashboard,
    section: "operations",
  },
  {
    path: "/provider/runtime",
    navKey: "providerRuntime",
    i18nKey: "sidebar.providerRuntime",
    icon: Activity,
    section: "operations",
  },
  {
    path: "/record",
    navKey: "record",
    i18nKey: "sidebar.record",
    icon: ClipboardList,
    section: "traffic",
  },
  {
    path: "/provider",
    navKey: "provider",
    i18nKey: "sidebar.provider",
    icon: Server,
    section: "resources",
  },
  {
    path: "/model",
    navKey: "model",
    i18nKey: "sidebar.model",
    icon: ArrowRightLeft,
    section: "resources",
  },
  {
    path: "/api_key",
    navKey: "apiKey",
    i18nKey: "sidebar.apiKey",
    icon: KeyRound,
    section: "resources",
  },
  {
    path: "/cost",
    navKey: "cost",
    i18nKey: "sidebar.cost",
    icon: DollarSign,
    section: "governance",
  },
  {
    path: "/security",
    navKey: "security",
    i18nKey: "sidebar.security",
    icon: ShieldCheck,
    section: "governance",
  },
];
