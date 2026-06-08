// Sky dome + endless ocean so there is no empty background when zoomed out.
// Tuned for a warm Levant daytime: a pale dusty-blue zenith fading to a hazy,
// faintly sun-warmed horizon band, over a sea that shades from deep teal at the
// horizon to a lighter shallow green near the shore. Both ride the camera in the
// game loop, so geometry stays centred on its own origin (no baked world offset).
import * as THREE from 'three';
import { WORLD_SIZE } from '../../shared/index.ts';

// Horizon haze colour. Also feeds scene.background + fog in SaladinGame, so this
// single value sets the warm milky distance the whole map dissolves into.
export const HORIZON = new THREE.Color('#d8d2be');

// Sky palette: cool dusty zenith, warm pale haze at the horizon, and a low warm
// sun glow biased toward the light direction so the dome doesn't read as a flat
// gradient. Kept subtle — this is backdrop, not a skybox showpiece.
const SKY_ZENITH = new THREE.Color('#5d8fc4');
const SKY_HAZE = HORIZON.clone();
const SUN_GLOW = new THREE.Color('#ffe9c2');
// Light direction matches the DirectionalLight in SaladinGame (40,70,20).
const SUN_DIR = new THREE.Vector3(40, 70, 20).normalize();

export function buildSky(): THREE.Mesh {
  const sky = new THREE.Mesh(
    new THREE.SphereGeometry(1200, 32, 16),
    new THREE.ShaderMaterial({
      side: THREE.BackSide,
      depthWrite: false,
      fog: false,
      uniforms: {
        zenith: { value: SKY_ZENITH.clone() },
        haze: { value: SKY_HAZE.clone() },
        sunGlow: { value: SUN_GLOW.clone() },
        sunDir: { value: SUN_DIR.clone() },
        exponent: { value: 0.62 },
      },
      vertexShader: /* glsl */ `
        varying vec3 vDir;
        void main() {
          vDir = normalize(position);
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }`,
      fragmentShader: /* glsl */ `
        uniform vec3 zenith; uniform vec3 haze; uniform vec3 sunGlow;
        uniform vec3 sunDir; uniform float exponent;
        varying vec3 vDir;
        void main() {
          vec3 dir = normalize(vDir);
          // Vertical gradient: haze at the horizon up to cool zenith.
          float t = pow(clamp(dir.y, 0.0, 1.0), exponent);
          vec3 col = mix(haze, zenith, t);
          // Warm sun bloom near the light direction, strongest low on the sky.
          float sun = pow(max(dot(dir, sunDir), 0.0), 6.0);
          float lowBias = 1.0 - smoothstep(0.0, 0.5, dir.y);
          col = mix(col, sunGlow, sun * (0.35 + 0.4 * lowBias));
          // Extra warmth hugging the horizon line for the dusty Levant feel.
          float horizonBand = 1.0 - smoothstep(0.0, 0.18, abs(dir.y));
          col = mix(col, haze, horizonBand * 0.25);
          gl_FragColor = vec4(col, 1.0);
        }`,
    })
  );
  sky.position.set(WORLD_SIZE / 2, 0, WORLD_SIZE / 2);
  sky.renderOrder = -1;
  return sky;
}

const OCEAN_DEEP = new THREE.Color('#1e5f86');
const OCEAN_SHALLOW = new THREE.Color('#3f93a8');

export function buildOcean(): THREE.Mesh {
  // Opaque — a see-through ocean reveals the bright sky beyond the finite
  // terrain, which reads as a hard square edge. Solid sea hides that seam.
  //
  // A shader fades deep teal at distance into a lighter shallow tone near the
  // map centre (under the camera), plus a soft band of horizon haze far out so
  // the sea melts into the sky instead of meeting it at a crisp line.
  const geo = new THREE.PlaneGeometry(8000, 8000, 1, 1);
  const mat = new THREE.ShaderMaterial({
    fog: true,
    uniforms: {
      deep: { value: OCEAN_DEEP.clone() },
      shallow: { value: OCEAN_SHALLOW.clone() },
      haze: { value: HORIZON.clone() },
      ...THREE.UniformsLib.fog,
    },
    vertexShader: /* glsl */ `
      #include <fog_pars_vertex>
      varying vec2 vXY;
      void main() {
        // Plane is built in its own XY then rotated flat; vXY in plane units.
        vXY = position.xy;
        vec4 mvPosition = modelViewMatrix * vec4(position, 1.0);
        gl_Position = projectionMatrix * mvPosition;
        #include <fog_vertex>
      }`,
    fragmentShader: /* glsl */ `
      #include <fog_pars_fragment>
      uniform vec3 deep; uniform vec3 shallow; uniform vec3 haze;
      varying vec2 vXY;
      void main() {
        // Distance from the plane centre (which tracks the camera) in plane units.
        float d = length(vXY);
        // Near the centre: lighter shallow water. Farther out: deep teal.
        float depthMix = smoothstep(20.0, 320.0, d);
        vec3 col = mix(shallow, deep, depthMix);
        // Faint banded shimmer so the flat sea isn't a dead colour field.
        float band = 0.5 + 0.5 * sin(vXY.x * 0.18) * sin(vXY.y * 0.18);
        col += (band - 0.5) * 0.04;
        // Far out, melt into the horizon haze to kill the visible plane edge.
        float far = smoothstep(800.0, 2600.0, d);
        col = mix(col, haze, far);
        gl_FragColor = vec4(col, 1.0);
        #include <fog_fragment>
      }`,
  });
  const ocean = new THREE.Mesh(geo, mat);
  ocean.rotation.x = -Math.PI / 2;
  ocean.position.set(WORLD_SIZE / 2, -0.05, WORLD_SIZE / 2);
  ocean.receiveShadow = false;
  return ocean;
}
