export const HOST =
  import.meta.env.VITE_SPACETIMEDB_HOST ?? 'ws://localhost:3000';
export const DB_NAME = import.meta.env.VITE_SPACETIMEDB_DB_NAME ?? 'saladin';
export const TOKEN_KEY = `${HOST}/${DB_NAME}/auth_token`;
