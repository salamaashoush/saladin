import { schema } from 'spacetimedb/server';
import {
  entity,
  unit,
  building,
  resourceNode,
  player,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
} from './tables.ts';

export const spacetimedb = schema({
  entity,
  unit,
  building,
  resourceNode,
  player,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
});

export default spacetimedb;
