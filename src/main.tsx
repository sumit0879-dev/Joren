import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';

// Diagnostic handlers for frontend runtime error reporting in CEF
window.addEventListener('error', (event) => {
  console.error('[JOREN UNHANDLED ERROR]', event.message, event.filename, event.lineno, event.colno, event.error);
});

window.addEventListener('unhandledrejection', (event) => {
  console.error('[JOREN UNHANDLED REJECTION]', event.reason);
});

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
