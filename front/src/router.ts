import { lazy, Component } from 'solid-js';

// Define a unified structure for route configuration
export interface RouteConfig {
  path: string;
  component: Component<any>; // Component for the route
  icon?: string;             // Optional: Icon for navigation
  text?: string;             // Optional: Text for navigation
  i18nKey?: string;          // Optional: i18n key for navigation text
}

// Lazy load page components
const RedirectToDashboard = lazy(() => import('./pages/RedirectToDashboard'));
const Login = lazy(() => import('./pages/Login'));
const Dashboard = lazy(() => import('./pages/Dashboard'));
const Record = lazy(() => import('./pages/Record'));
const Provider = lazy(() => import('./pages/Provider'));
const ProviderEdit = lazy(() => import('./pages/ProviderEdit')); // Import for ProviderEdit page
const ModelEdit = lazy(() => import('./pages/ModelEdit'));
const ApiKeyPage = lazy(() => import('./pages/ApiKey')); // Added import for ApiKeyPage
const ModelAlias = lazy(() => import('./pages/ModelAlias'));
const AccessControlPage = lazy(() => import('./pages/AccessControlPage'));
const CustomFields = lazy(() => import('./pages/CustomFields'));
const Price = lazy(() => import('./pages/Price'));

// Separate configuration for the Login route (outside the main layout)
export const loginRoute: RouteConfig = {
  path: "/login",
  component: Login
};

// Unified array for main application routes that are part of the PageWrapper layout
export const mainRoutes: RouteConfig[] = [
  { path: "/", component: RedirectToDashboard },
  { path: "/dashboard", component: Dashboard, icon: "📊", text: "Dashboard", i18nKey: "sidebar.dashboard" },
  { path: "/record", component: Record, icon: " M", text: "Record", i18nKey: "sidebar.record" },
  { path: "/provider", component: Provider, icon: " P", text: "Provider", i18nKey: "sidebar.provider" },
  { path: "/provider/new", component: ProviderEdit }, // Route for new provider
  { path: "/provider/edit/:id", component: ProviderEdit }, // Route for editing provider
  { path: "/model/edit/:id", component: ModelEdit },
  { path: "/api_key", component: ApiKeyPage, icon: "🔑", text: "API Key", i18nKey: "sidebar.apiKey" }, // Added API Key route
  { path: "/model_transform", component: ModelAlias, icon: " T", text: "Model Transform", i18nKey: "sidebar.modelTransform" },
  { path: "/access_control", component: AccessControlPage, icon: " L", text: "Access Control", i18nKey: "sidebar.accessControlPolicy" },
  { path: "/custom_fields", component: CustomFields, icon: " C", text: "Custom Fields", i18nKey: "sidebar.customFields" },
  { path: "/price", component: Price, icon: "💲", text: "Price", i18nKey: "sidebar.price" },
  { path: "*", component: RedirectToDashboard },
];

// Helper function to get routes intended for the PageWrapper layout (those with nav items)
// This might not be strictly necessary if App.tsx and PageWrapper.tsx filter directly,
// but can be kept for clarity or potential future use.
export const getWrappedRoutes = (): RouteConfig[] => {
  // Routes within PageWrapper are essentially the mainRoutes themselves in this structure
  return mainRoutes;
};

// Helper function to get only the navigation items from mainRoutes
export const getNavItems = (): RouteConfig[] => {
  // Include items that have an icon and EITHER an i18nKey OR a text property.
  // This makes it more robust for i18n.
  return mainRoutes.filter(route => route.icon && (route.i18nKey || route.text));
};
