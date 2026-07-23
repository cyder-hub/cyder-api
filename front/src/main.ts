import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import pinia from "./store";
import i18n from "./i18n";
import { startAuthCoordination } from "./services/auth";
import "./style.css";

const app = createApp(App);

app.use(pinia);
startAuthCoordination();
app.use(router);
app.use(i18n);

app.mount("#app");
