import type { ReducerCtx } from 'spacetimedb/server';
import { spacetimedb } from './db.ts';

export type Ctx = ReducerCtx<typeof spacetimedb.schemaType>;
