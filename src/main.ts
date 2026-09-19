import { createApp } from "vue";
import "lapstyle/index.css";
import { enhance } from "lapstyle";
import App from "./App.vue";
import "./styles/app.css";
import { bindWebviewZoom } from "./zoom";

bindWebviewZoom();

const app = createApp(App);
app.mount("#app");
enhance(document);
