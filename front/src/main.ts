import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import pinia from "./store";
import i18n from "./i18n";
import "./style.css";

const app = createApp(App);

app.use(router);
app.use(pinia);
app.use(i18n);

app.mount("#app");
