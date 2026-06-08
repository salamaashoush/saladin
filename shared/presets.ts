// DATA catalog of map presets. A preset biases worldgen: it nudges the sea level
// and the moisture banding so the same generator yields a recognizably different
// land — a wet "Verdant" map, a parched "Desert", a sea-broken "Archipelago", a
// rugged "Highlands". Pure data; terrain.ts reads a MapBias and shifts its
// thresholds. The module stores the chosen preset id alongside the seed so the
// client recomputes identical land.
//
// Biasing the SAME classify() (rather than forking generators) keeps one source
// of truth for biome bands and guarantees module/client agreement.

export interface MapBias {
  seaShift: number; // + raises the waterline (more sea), - lowers it (more land)
  moistShift: number; // + wetter (grass/forest), - drier (desert/dunes)
  elevGain: number; // multiplies land relief — higher = taller hills/mountains
}

export interface MapPreset {
  id: string;
  label: string;
  description: string;
  bias: MapBias;
}

export const NEUTRAL_BIAS: MapBias = { seaShift: 0, moistShift: 0, elevGain: 1 };

export const MAP_PRESETS: MapPreset[] = [
  {
    id: 'continental',
    label: 'Continental',
    description: 'Balanced land with a fair mix of every biome.',
    bias: NEUTRAL_BIAS,
  },
  {
    id: 'verdant',
    label: 'Verdant',
    description: 'Wet, fertile country — broad grassland and deep forest.',
    bias: { seaShift: -0.02, moistShift: 0.16, elevGain: 0.9 },
  },
  {
    id: 'desert',
    label: 'Arabian Desert',
    description: 'Parched dunes and sand, sparse oases by the water.',
    bias: { seaShift: 0.0, moistShift: -0.2, elevGain: 0.85 },
  },
  {
    id: 'highlands',
    label: 'Highlands',
    description: 'Rugged uplands — towering hills, mountains, and snow.',
    bias: { seaShift: -0.03, moistShift: 0.02, elevGain: 1.45 },
  },
  {
    id: 'archipelago',
    label: 'Archipelago',
    description: 'A sea of scattered islands — control the straits.',
    bias: { seaShift: 0.1, moistShift: 0.06, elevGain: 1.0 },
  },
];

export function mapPresetById(id: string): MapPreset {
  return MAP_PRESETS.find((p) => p.id === id) ?? MAP_PRESETS[0];
}

export function mapPresetByIndex(index: number): MapPreset {
  return MAP_PRESETS[((index % MAP_PRESETS.length) + MAP_PRESETS.length) % MAP_PRESETS.length];
}

export function biasOf(presetId: string): MapBias {
  return mapPresetById(presetId).bias;
}
