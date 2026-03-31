import { h } from "vue";
import {
  LayoutDashboard,
  KeyRound,
  ArrowRightLeft,
  Shield,
  Type,
  DollarSign,
  ClipboardList,
  Server,
} from "lucide-vue-next";

export const navItems = [
  {
    path: "/dashboard",
    i18nKey: "sidebar.dashboard",
    icon: h(LayoutDashboard),
  },
  {
    path: "/record",
    i18nKey: "sidebar.record",
    icon: h(ClipboardList),
  },
  {
    path: "/provider",
    i18nKey: "sidebar.provider",
    icon: h(Server),
  },
  {
    path: "/api_key",
    i18nKey: "sidebar.apiKey",
    icon: h(KeyRound),
  },
  {
    path: "/model_transform",
    i18nKey: "sidebar.modelTransform",
    icon: h(ArrowRightLeft),
  },
  {
    path: "/access_control",
    i18nKey: "sidebar.accessControlPolicy",
    icon: h(Shield),
  },
  {
    path: "/custom_fields",
    i18nKey: "sidebar.customFields",
    icon: h(Type),
  },
  {
    path: "/price",
    i18nKey: "sidebar.price",
    icon: h(DollarSign),
  },
];
