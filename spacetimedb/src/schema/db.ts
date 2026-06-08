import { schema } from 'spacetimedb/server';
import {
  entity,
  unit,
  building,
  garrison,
  resourceNode,
  player,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
  economyTimer,
} from './tables.ts';

export const spacetimedb = schema({
  entity,
  unit,
  building,
  garrison,
  resourceNode,
  player,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
  economyTimer,
});

export default spacetimedb;
