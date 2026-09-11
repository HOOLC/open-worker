import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { App } from "./app/App";
import "./styles/app.css";
const legacy: Record<string, string> = {
  identity: "/brand",
  product: "/product",
  components: "/pc/button",
  mobile: "/mobile",
  avatars: "/brand",
  scenes: "/brand",
  inventory: "/materials",
  delivery: "/materials",
};
const hash = location.hash.slice(1);
if (legacy[hash]) history.replaceState(null, "", "#" + legacy[hash]);
createRoot(document.getElementById("root")!).render(
  <HashRouter>
    <App />
  </HashRouter>,
);
