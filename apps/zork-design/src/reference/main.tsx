import { createRoot } from "react-dom/client";
import { ReferenceApp } from "./ReferenceApp";
import "./reference.css";
import "./approved.css";
document.documentElement.dataset.designStyle =
  new URLSearchParams(location.search).get("style") === "legacy" ? "legacy" : "approved";
createRoot(document.getElementById("root")!).render(<ReferenceApp />);

if (new URLSearchParams(location.search).get("isolate") === "1") document.body.classList.add("reference-isolated");
