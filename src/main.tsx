import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import './ui/theme.css';
import App from './App.tsx';
import { Identity } from 'spacetimedb';
import { SpacetimeDBProvider } from 'spacetimedb/react';
import { DbConnection, ErrorContext } from './module_bindings/index.ts';
import { HOST, DB_NAME, TOKEN_KEY } from './config.ts';

const onConnect = (_conn: DbConnection, identity: Identity, token: string) => {
  localStorage.setItem(TOKEN_KEY, token);
  console.log('Connected to SpacetimeDB:', identity.toHexString());
};

const onConnectError = (_ctx: ErrorContext, err: Error) => {
  console.error('Error connecting to SpacetimeDB:', err);
};

const connectionBuilder = DbConnection.builder()
  .withUri(HOST)
  .withDatabaseName(DB_NAME)
  .withToken(localStorage.getItem(TOKEN_KEY) || undefined)
  .onConnect(onConnect)
  .onConnectError(onConnectError);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <SpacetimeDBProvider connectionBuilder={connectionBuilder}>
      <App />
    </SpacetimeDBProvider>
  </StrictMode>
);
