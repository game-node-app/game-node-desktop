import React from 'react';
import ReactDOM from 'react-dom/client';
import { AppProviders } from './AppProviders';
import { App } from './App';
import './styles/theme.css';

const rootElement = document.getElementById('root');
if (rootElement) {
  ReactDOM.createRoot(rootElement).render(
    <React.StrictMode>
      <AppProviders>
        <App />
      </AppProviders>
    </React.StrictMode>,
  );
}
