import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

// 应用入口：挂载根组件（无状态库，页面状态由 App.vue 持有）
createApp(App).mount("#app");
