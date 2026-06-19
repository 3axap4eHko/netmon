import React from 'react';
import { createRoot } from 'react-dom/client';
import { Dashboard } from './components/Dashboard';
import { hydrateSettings } from './storage';
import './styles.css';

hydrateSettings().finally(() => {
  const root = createRoot(document.getElementById('root')!);
  root.render(<Dashboard />);
});
