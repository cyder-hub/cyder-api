// Define a unified structure for navigation item configuration
export interface NavItemConfig {
  path: string;
  icon?: string;             // Optional: Icon for navigation
  text?: string;             // Optional: Text for navigation
  i18nKey?: string;          // Optional: i18n key for navigation text
}

// Array for navigation items displayed in the sidebar
const navItems: NavItemConfig[] = [
  { path: "/dashboard", icon: "📊", text: "Dashboard", i18nKey: "sidebar.dashboard" },
  { path: "/record", icon: " M", text: "Record", i18nKey: "sidebar.record" },
  { path: "/provider", icon: " P", text: "Provider", i18nKey: "sidebar.provider" },
  { path: "/api_key", icon: "🔑", text: "API Key", i18nKey: "sidebar.apiKey" },
  { path: "/model_transform", icon: " T", text: "Model Transform", i18nKey: "sidebar.modelTransform" },
  { path: "/access_control", icon: " L", text: "Access Control", i18nKey: "sidebar.accessControlPolicy" },
  { path: "/custom_fields", icon: " C", text: "Custom Fields", i18nKey: "sidebar.customFields" },
  { path: "/price", icon: "💲", text: "Price", i18nKey: "sidebar.price" },
];

// Helper function to get the navigation items
export const getNavItems = (): NavItemConfig[] => {
  return navItems;
};
