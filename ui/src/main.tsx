import React from 'react';
import ReactDOM from 'react-dom/client';
import { AppProviders } from './AppProviders';
import { App } from './App';
import { ErrorBoundary, installLogCapture } from './diagnostics';
import './styles/theme.css';

// Keep recent console output and uncaught failures, so a crash report can include them.
installLogCapture();

const rootElement = document.getElementById('root');
if (rootElement) {
  ReactDOM.createRoot(rootElement).render(
    <React.StrictMode>
      <AppProviders>
        <ErrorBoundary scope="app" variant="page">
          <App />
        </ErrorBoundary>
      </AppProviders>
    </React.StrictMode>,
  );
}
