import { schema } from 'spacetimedb/server';
import {
  entity,
  unit,
  building,
  garrison,
  resourceNode,
  player,
  research,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
  economyTimer,
  researchTimer,
} from './tables.ts';

export const spacetimedb = schema({
  entity,
  unit,
  building,
  garrison,
  resourceNode,
  player,
  research,
  config,
  shot,
  ai,
  moveTimer,
  aiTimer,
  combatTimer,
  aiBrainTimer,
  economyTimer,
  researchTimer,
});

export default spacetimedb;
