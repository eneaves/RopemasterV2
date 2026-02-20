import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import './styles/globals.css';
import { LicenseProvider } from "./providers/LicenseProvider";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LicenseProvider>
      <App />
    </LicenseProvider>
  </React.StrictMode>,
);
