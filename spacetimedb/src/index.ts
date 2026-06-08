// Saladin — authoritative RTS simulation (SpacetimeDB TS module).
//
// This entry is a thin barrel: the single schema lives in ./schema/db.ts and is
// re-exported as the default below; every reducer and lifecycle hook is
// re-exported BY NAME so the host registers it (the export name becomes the
// reducer name — a reducer that is not re-exported here vanishes silently).
//
// Table schemas register only via the one schema({...}) call in ./schema/db.ts;
// all reducer/system files build off that same instance.
export { default } from './schema/db.ts';

export * from './lifecycle.ts';

export * from './reducers/match.ts';
export * from './reducers/unit_commands.ts';
export * from './reducers/build_commands.ts';

export * from './systems/movement.ts';
export * from './systems/gather_ai.ts';
export * from './systems/combat.ts';
export * from './systems/ai_brain.ts';
