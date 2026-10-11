import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";

// Files from the desktop go to the app, not the page (see lib/dropped), but
// only if the page lets the drop happen: refused here, the system cancels
// the drag and the app never hears of it. Anything else, such as a picture
// dragged within the page, is refused; dropped, the webview would open it in
// place of the app. The drop itself is always kept from the page.
window.addEventListener("dragover", (event) => {
  event.preventDefault();
  const types = event.dataTransfer?.types ?? [];
  if (event.dataTransfer) event.dataTransfer.dropEffect = types.includes("Files") || types.includes("text/uri-list") ? "copy" : "none";
});
window.addEventListener("drop", (event) => event.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
