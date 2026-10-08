import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";

// A file dropped where nothing takes it would otherwise replace the app with
// the file itself: the webview opens whatever lands on it. The conversation
// takes files by preventing the default first, so only the rest is refused.
window.addEventListener("dragover", (event) => {
  if (event.defaultPrevented) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "none";
});
window.addEventListener("drop", (event) => event.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
