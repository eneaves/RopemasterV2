import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import './styles/globals.css';
import { LicenseProvider } from "./providers/LicenseProvider";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <BrowserRouter>
      <LicenseProvider>
        <App />
      </LicenseProvider>
    </BrowserRouter>
  </React.StrictMode>,
);
