// Sky dome + endless ocean so there is no empty background when zoomed out.
import * as THREE from 'three';
import { WORLD_SIZE } from '../../shared/index.ts';

export const HORIZON = new THREE.Color('#bcd8ea');

export function buildSky(): THREE.Mesh {
  const sky = new THREE.Mesh(
    new THREE.SphereGeometry(1200, 32, 16),
    new THREE.ShaderMaterial({
      side: THREE.BackSide,
      depthWrite: false,
      uniforms: {
        top: { value: new THREE.Color('#5a93c8') },
        bottom: { value: HORIZON.clone() },
        exponent: { value: 0.7 },
      },
      vertexShader: /* glsl */ `
        varying vec3 vDir;
        void main() {
          vDir = normalize(position);
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }`,
      fragmentShader: /* glsl */ `
        uniform vec3 top; uniform vec3 bottom; uniform float exponent;
        varying vec3 vDir;
        void main() {
          float t = pow(max(vDir.y, 0.0), exponent);
          gl_FragColor = vec4(mix(bottom, top, t), 1.0);
        }`,
    })
  );
  sky.position.set(WORLD_SIZE / 2, 0, WORLD_SIZE / 2);
  sky.renderOrder = -1;
  return sky;
}

export function buildOcean(): THREE.Mesh {
  // Opaque — a see-through ocean reveals the bright sky beyond the finite
  // terrain, which reads as a hard square edge. Solid sea hides that seam.
  const ocean = new THREE.Mesh(
    new THREE.PlaneGeometry(8000, 8000, 1, 1),
    new THREE.MeshStandardMaterial({
      color: 0x2a6b95,
      roughness: 0.35,
      metalness: 0.2,
    })
  );
  ocean.rotation.x = -Math.PI / 2;
  ocean.position.set(WORLD_SIZE / 2, -0.05, WORLD_SIZE / 2);
  ocean.receiveShadow = false;
  return ocean;
}
