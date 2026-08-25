/**
 * 3D Holographic Avatar Engine (Three.js)
 * Avatars (3D shape) and Color Themes (palette) are independently selectable.
 * Avatars:
 * 1. hAlcy: Harmonic Particle Lattice & Orbital Rings
 * 2. A.R.X.LIMES: Floating 3D Voxel Hypercubes & Crystalline Monolith
 * 3. The Nexus: Inward Falling Particles into a Gravitational Singularity
 * 4. R.E.D. 9000 (HAL 9000): Central Glowing Sphere with Two Orbit Circles
 * 5. A.R.X.LOGOS: Jagged Geometric Star with Neon Particle Burst
 * 6. Real-time Audio Frequency and State deformation.
 *
 * Color Themes: halcy (cyan), nexus (green), arx-limes (amber), arx-logos (magenta), red (crimson)
 * Any color theme can be applied to any avatar shape.
 */

const THEME_PALETTES = {
    halcy:      { r: 0.0, g: 0.9, b: 1.0, hex: 0x00f0ff, hex2: 0x00a2ff, hex3: 0xb026ff },
    nexus:      { r: 0.1, g: 1.0, b: 0.4, hex: 0x00ff66, hex2: 0x00cc55, hex3: 0x00ffaa },
    'arx-limes': { r: 1.0, g: 0.67, b: 0.0, hex: 0xffaa00, hex2: 0xff5500, hex3: 0xff3300 },
    'arx-logos': { r: 0.88, g: 0.14, b: 0.76, hex: 0xe024c3, hex2: 0x9d00ff, hex3: 0xff00ff },
    red:        { r: 1.0, g: 0.07, b: 0.13, hex: 0xff1133, hex2: 0x0066ff, hex3: 0x00f0ff }
};

class HologramAvatar {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.state = 'IDLE'; // IDLE, LISTENING, THINKING, SPEAKING
        this.currentAvatar = 'halcy'; // halcy, arx-limes, nexus, red, arx-logos
        this.currentColorTheme = 'halcy'; // halcy, nexus, arx-limes, arx-logos, red
        this.activePalette = THEME_PALETTES.halcy;
        this.audioData = new Uint8Array(64);

        this.scene = null;
        this.camera = null;
        this.renderer = null;

        // 1. hAlcy structures
        this.particleSystem = null;
        this.particleCount = 2800;
        this.basePositions = [];
        this.coreOrb = null; // obsidian inner sphere — fixed color, not theme-tinted
        this.halcyOuterRing = null; // static cyan ring — fixed color, not theme-tinted
        this.halcyInnerRingGroup = null; // ultramarine equalizer ring — fixed color, not theme-tinted
        this.halcyInnerSegments = [];

        // 2. A.R.X.LIMES structures
        this.arxLimesGroup = null;
        this.arxLimesCubes = [];
        this.arxLimesCore = null;
        this.arxLimesCrystalMat = null;
        this.arxLimesCubeMat = null;

        // 3. The Nexus Matrix Singularity structures
        this.nexusGroup = null;
        this.nexusParticles = null;
        this.nexusParticleCount = 1400;
        this.nexusData = [];
        this.nexusSingularity = null;
        this.nexusSingularityGlow = null;

        // 4. R.E.D. 9000 / HAL 9000 structures (Central Sphere + Two Orbit Circles)
        this.redGroup = null;
        this.redCoreSphere = null;
        this.redLensOuter = null;
        this.redPupil = null;
        this.redBlueCircle = null;
        this.redCyanCircle = null;

        // 5. A.R.X.LOGOS - Jagged Geometric Star
        this.arxLogosGroup = null;
        this.arxLogosStarPoints = [];
        this.arxLogosInnerCore = null;
        this.arxLogosRings = [];
        this.arxLogosSpikeMat = null;

        this.clock = null;
        this.mouseX = 0;
        this.mouseY = 0;

        this.init();
    }

    init() {
        if (!this.container || typeof THREE === 'undefined') {
            console.error("Three.js or container not available");
            return;
        }

        const width = this.container.clientWidth || 400;
        const height = this.container.clientHeight || 400;

        this.scene = new THREE.Scene();
        this.camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 1000);
        this.camera.position.z = 240;

        // Lights only affect the hAlcy obsidian core (MeshPhongMaterial) — every other avatar
        // uses MeshBasicMaterial, which ignores scene lighting entirely.
        this.scene.add(new THREE.AmbientLight(0x30304a, 1.4));
        const obsidianHighlight = new THREE.PointLight(0x9fb4ff, 2.4, 700);
        obsidianHighlight.position.set(90, 130, 220);
        this.scene.add(obsidianHighlight);

        this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
        this.renderer.setSize(width, height);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.container.innerHTML = '';
        this.container.appendChild(this.renderer.domElement);

        this.clock = new THREE.Clock();

        // Build all avatar architectures
        this.buildHalcyAvatar();
        this.buildArxLimesAvatar();
        this.buildNexusAvatar();
        this.buildRed9000Avatar();
        this.buildArxLogosAvatar();

        // Initial avatar shape + color theme setup (independent of each other)
        this.setAvatar(this.currentAvatar);
        this.setColorTheme(this.currentColorTheme);

        window.addEventListener('mousemove', (e) => {
            this.mouseX = (e.clientX - window.innerWidth / 2) * 0.0005;
            this.mouseY = (e.clientY - window.innerHeight / 2) * 0.0005;
        });

        window.addEventListener('resize', () => this.onResize());

        this.animate();
    }

    // Shared soft glow sprite texture (theme-neutral white so vertex colors tint it cleanly)
    createGlowSpriteTexture(size = 32) {
        const canvas = document.createElement('canvas');
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext('2d');
        const gradient = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
        gradient.addColorStop(0, 'rgba(255,255,255,1)');
        gradient.addColorStop(0.3, 'rgba(255,255,255,0.8)');
        gradient.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = gradient;
        ctx.fillRect(0, 0, size, size);
        return new THREE.CanvasTexture(canvas);
    }

    buildHalcyAvatar() {
        const geometry = new THREE.BufferGeometry();
        const positions = new Float32Array(this.particleCount * 3);
        const colors = new Float32Array(this.particleCount * 3);
        const radius = 34;
        this.halcyLatticeRadius = radius;

        for (let i = 0; i < this.particleCount; i++) {
            const phi = Math.acos(-1 + (2 * i) / this.particleCount);
            const theta = Math.sqrt(this.particleCount * Math.PI) * phi;

            const x = radius * Math.cos(theta) * Math.sin(phi);
            const y = radius * Math.sin(theta) * Math.sin(phi);
            const z = radius * Math.cos(phi);

            positions[i * 3] = x;
            positions[i * 3 + 1] = y;
            positions[i * 3 + 2] = z;

            this.basePositions.push({ x, y, z });

            colors[i * 3] = 0.0;
            colors[i * 3 + 1] = 0.85 + Math.random() * 0.15;
            colors[i * 3 + 2] = 1.0;
        }

        geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

        const texture = this.createGlowSpriteTexture(32);

        const material = new THREE.PointsMaterial({
            size: 3.2,
            vertexColors: true,
            map: texture,
            transparent: true,
            opacity: 0.9,
            blending: THREE.AdditiveBlending,
            depthWrite: false
        });

        this.particleSystem = new THREE.Points(geometry, material);
        this.scene.add(this.particleSystem);

        // Obsidian inner sphere — solid glossy dark core, fixed color regardless of color theme
        const coreGeom = new THREE.SphereGeometry(18, 32, 32);
        const coreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0a0f,
            specular: 0x8fa8ff,
            shininess: 90,
            transparent: true,
            opacity: 0.97
        });
        this.coreOrb = new THREE.Mesh(coreGeom, coreMat);
        this.scene.add(this.coreOrb);

        // Static outer ring — fixed cyan, unaffected by color theme
        const outerRingGeom = new THREE.RingGeometry(76, 78.5, 64);
        const outerRingMat = new THREE.MeshBasicMaterial({
            color: 0x00f0ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.5,
            blending: THREE.AdditiveBlending
        });
        this.halcyOuterRing = new THREE.Mesh(outerRingGeom, outerRingMat);
        this.halcyOuterRing.rotation.x = 0.6;
        this.halcyOuterRing.rotation.y = 0.2;
        this.halcyOuterRing.userData = { speed: 0.012, baseRotX: 0.6, baseRotY: 0.2 };
        this.scene.add(this.halcyOuterRing);

        // Inner ultramarine equalizer ring — segmented so its circumference can "thicken" per-bar
        // like an audio equalizer while speaking, and the whole ring can sway on its Z axis.
        this.halcyInnerRingGroup = new THREE.Group();
        this.halcyInnerRingGroup.userData = { speed: 0.01 };
        const innerRingRadius = 58;
        const segmentCount = 40;
        const segMat = new THREE.MeshBasicMaterial({
            color: 0x2b3eff,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        for (let i = 0; i < segmentCount; i++) {
            const angle = (i / segmentCount) * Math.PI * 2;
            const segGeom = new THREE.BoxGeometry(2.4, 6, 1.6);
            segGeom.translate(0, 3, 0); // pivot at inner edge so it only extends outward when scaled
            const seg = new THREE.Mesh(segGeom, segMat);
            seg.position.set(Math.cos(angle) * innerRingRadius, Math.sin(angle) * innerRingRadius, 0);
            seg.rotation.z = angle - Math.PI / 2;
            seg.userData = { angle };
            this.halcyInnerSegments.push(seg);
            this.halcyInnerRingGroup.add(seg);
        }
        this.scene.add(this.halcyInnerRingGroup);
    }

    buildArxLimesAvatar() {
        this.arxLimesGroup = new THREE.Group();

        const coreGeom = new THREE.BoxGeometry(28, 28, 28);
        const coreMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00,
            wireframe: true,
            transparent: true,
            opacity: 0.8,
            blending: THREE.AdditiveBlending
        });
        this.arxLimesCore = new THREE.Mesh(coreGeom, coreMat);
        this.arxLimesGroup.add(this.arxLimesCore);

        const crystalGeom = new THREE.OctahedronGeometry(14, 0);
        this.arxLimesCrystalMat = new THREE.MeshBasicMaterial({
            color: 0xff5500,
            transparent: true,
            opacity: 0.9,
            blending: THREE.AdditiveBlending
        });
        const crystal = new THREE.Mesh(crystalGeom, this.arxLimesCrystalMat);
        this.arxLimesCore.add(crystal);

        const cubeGeom = new THREE.BoxGeometry(8, 8, 8);
        this.arxLimesCubeMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00,
            wireframe: true,
            transparent: true,
            opacity: 0.65,
            blending: THREE.AdditiveBlending
        });

        const numCubes = 54;
        for (let i = 0; i < numCubes; i++) {
            const cube = new THREE.Mesh(cubeGeom, this.arxLimesCubeMat);
            const radius = 45 + (i % 3) * 22;
            const phi = Math.acos(-1 + (2 * i) / numCubes);
            const theta = Math.sqrt(numCubes * Math.PI) * phi;

            const bx = radius * Math.cos(theta) * Math.sin(phi);
            const by = radius * Math.sin(theta) * Math.sin(phi);
            const bz = radius * Math.cos(phi);

            cube.position.set(bx, by, bz);
            cube.userData = {
                baseX: bx,
                baseY: by,
                baseZ: bz,
                rotSpeedX: (Math.random() - 0.5) * 0.04,
                rotSpeedY: (Math.random() - 0.5) * 0.04,
                radius: radius
            };
            this.arxLimesCubes.push(cube);
            this.arxLimesGroup.add(cube);
        }

        this.scene.add(this.arxLimesGroup);
    }

    buildNexusAvatar() {
        this.nexusGroup = new THREE.Group();

        // 1. Singular Gravitational Core Point
        const singGeom = new THREE.SphereGeometry(6, 32, 32);
        const singMat = new THREE.MeshBasicMaterial({
            color: 0xffffff,
            transparent: true,
            opacity: 1.0,
            blending: THREE.AdditiveBlending
        });
        this.nexusSingularity = new THREE.Mesh(singGeom, singMat);
        this.nexusGroup.add(this.nexusSingularity);

        const haloGeom = new THREE.SphereGeometry(14, 32, 32);
        const haloMat = new THREE.MeshBasicMaterial({
            color: 0x00ff66,
            wireframe: true,
            transparent: true,
            opacity: 0.45,
            blending: THREE.AdditiveBlending
        });
        this.nexusSingularityGlow = new THREE.Mesh(haloGeom, haloMat);
        this.nexusGroup.add(this.nexusSingularityGlow);

        const matrixTexture = this.createGlowSpriteTexture(64);

        const geometry = new THREE.BufferGeometry();
        const positions = new Float32Array(this.nexusParticleCount * 3);
        const colors = new Float32Array(this.nexusParticleCount * 3);

        for (let i = 0; i < this.nexusParticleCount; i++) {
            const radius = 60 + Math.random() * 110;
            const angle = Math.random() * Math.PI * 2;
            const y = (Math.random() - 0.5) * 160;

            const x = radius * Math.cos(angle);
            const z = radius * Math.sin(angle);

            positions[i * 3] = x;
            positions[i * 3 + 1] = y;
            positions[i * 3 + 2] = z;

            this.nexusData.push({
                radius: radius,
                angle: angle,
                y: y,
                fallSpeed: 0.8 + Math.random() * 1.5,
                spiralSpeed: 0.02 + Math.random() * 0.03,
                inwardSpeed: 0.4 + Math.random() * 0.6
            });

            colors[i * 3] = 0.1;
            colors[i * 3 + 1] = 0.9 + Math.random() * 0.1;
            colors[i * 3 + 2] = 0.4;
        }

        geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

        const mat = new THREE.PointsMaterial({
            size: 4.5,
            vertexColors: true,
            map: matrixTexture,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending,
            depthWrite: false
        });

        this.nexusParticles = new THREE.Points(geometry, mat);
        this.nexusGroup.add(this.nexusParticles);

        this.scene.add(this.nexusGroup);
    }

    buildRed9000Avatar() {
        this.redGroup = new THREE.Group();

        // 1. Central Glowing Sphere (HAL / Reactive Daemon Eye)
        const coreGeom = new THREE.SphereGeometry(26, 32, 32);
        const coreMat = new THREE.MeshBasicMaterial({
            color: 0xff1133,
            transparent: true,
            opacity: 0.92,
            blending: THREE.AdditiveBlending
        });
        this.redCoreSphere = new THREE.Mesh(coreGeom, coreMat);
        this.redGroup.add(this.redCoreSphere);

        // Outer Tactical Wireframe Lens Shell
        const lensGeom = new THREE.SphereGeometry(34, 24, 24);
        const lensMat = new THREE.MeshBasicMaterial({
            color: 0xff3355,
            wireframe: true,
            transparent: true,
            opacity: 0.4,
            blending: THREE.AdditiveBlending
        });
        this.redLensOuter = new THREE.Mesh(lensGeom, lensMat);
        this.redGroup.add(this.redLensOuter);

        // Center intense white pupil
        const pupilGeom = new THREE.SphereGeometry(8, 16, 16);
        const pupilMat = new THREE.MeshBasicMaterial({
            color: 0xffffff,
            transparent: true,
            opacity: 0.95
        });
        this.redPupil = new THREE.Mesh(pupilGeom, pupilMat);
        this.redGroup.add(this.redPupil);

        // 2. First Orbit Circle (Ring 1)
        const blueGeom = new THREE.RingGeometry(64, 67, 64);
        const blueMat = new THREE.MeshBasicMaterial({
            color: 0x0066ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        this.redBlueCircle = new THREE.Mesh(blueGeom, blueMat);
        this.redBlueCircle.rotation.x = 1.1;
        this.redBlueCircle.rotation.y = 0.3;
        this.redBlueCircle.userData = { speed: 0.018 };
        this.redGroup.add(this.redBlueCircle);

        // 3. Second Orbit Circle (Ring 2)
        const cyanGeom = new THREE.RingGeometry(84, 87, 64);
        const cyanMat = new THREE.MeshBasicMaterial({
            color: 0x00f0ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        this.redCyanCircle = new THREE.Mesh(cyanGeom, cyanMat);
        this.redCyanCircle.rotation.x = -0.7;
        this.redCyanCircle.rotation.y = 0.9;
        this.redCyanCircle.userData = { speed: -0.014 };
        this.redGroup.add(this.redCyanCircle);

        this.scene.add(this.redGroup);
    }

    buildArxLogosAvatar() {
        this.arxLogosGroup = new THREE.Group();

        // Jagged Geometric Star — built from sharp spike tetrahedra radiating from center
        this.arxLogosSpikeMat = new THREE.MeshBasicMaterial({
            color: 0xe024c3,
            wireframe: true,
            transparent: true,
            opacity: 0.90,
            blending: THREE.AdditiveBlending
        });

        const spikeCount = 12;
        for (let i = 0; i < spikeCount; i++) {
            // Each spike is a narrow cone (tetrahedron) pointing outward
            const height = 55 + (i % 3) * 20; // jagged varying lengths
            const coneGeom = new THREE.ConeGeometry(6 + (i % 3) * 3, height, 4); // 4 sides = diamond spike
            const spike = new THREE.Mesh(coneGeom, this.arxLogosSpikeMat);

            // Distribute spikes in a star sphere pattern
            const phi = Math.acos(-1 + (2 * i) / spikeCount);
            const theta = Math.sqrt(spikeCount * Math.PI) * phi;

            const x = Math.cos(theta) * Math.sin(phi);
            const y = Math.sin(theta) * Math.sin(phi);
            const z = Math.cos(phi);

            // Position spike tip at its direction, rotated to point outward
            spike.position.set(x * height * 0.55, y * height * 0.55, z * height * 0.55);

            // Align cone to point outward along its position vector
            spike.lookAt(x * 200, y * 200, z * 200);
            spike.rotateX(Math.PI / 2);

            spike.userData = {
                baseX: x * height * 0.55,
                baseY: y * height * 0.55,
                baseZ: z * height * 0.55,
                phase: (i / spikeCount) * Math.PI * 2,
                len: height
            };

            this.arxLogosStarPoints.push(spike);
            this.arxLogosGroup.add(spike);
        }

        // Inner dense core — compressed icosahedron
        const coreGeom = new THREE.IcosahedronGeometry(16, 1);
        const coreMat = new THREE.MeshBasicMaterial({
            color: 0xff00ff,
            wireframe: true,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        this.arxLogosInnerCore = new THREE.Mesh(coreGeom, coreMat);
        this.arxLogosGroup.add(this.arxLogosInnerCore);

        // Two thin orbit rings around the star
        const ring1Geom = new THREE.RingGeometry(90, 92, 48);
        const ring1Mat = new THREE.MeshBasicMaterial({
            color: 0xe024c3,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.55,
            blending: THREE.AdditiveBlending
        });
        const arxLogosRing1 = new THREE.Mesh(ring1Geom, ring1Mat);
        arxLogosRing1.rotation.x = 0.5;
        arxLogosRing1.userData = { speed: 0.022 };
        this.arxLogosRings.push(arxLogosRing1);
        this.arxLogosGroup.add(arxLogosRing1);

        const ring2Geom = new THREE.RingGeometry(110, 112, 48);
        const ring2Mat = new THREE.MeshBasicMaterial({
            color: 0x9d00ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.45,
            blending: THREE.AdditiveBlending
        });
        const arxLogosRing2 = new THREE.Mesh(ring2Geom, ring2Mat);
        arxLogosRing2.rotation.x = -0.9;
        arxLogosRing2.rotation.y = 0.6;
        arxLogosRing2.userData = { speed: -0.016 };
        this.arxLogosRings.push(arxLogosRing2);
        this.arxLogosGroup.add(arxLogosRing2);

        this.scene.add(this.arxLogosGroup);
    }

    // Which 3D shape is visible / animated. Independent of color theme.
    setAvatar(avatar) {
        this.currentAvatar = avatar;
        const isArxLimes = avatar === 'arx-limes';
        const isNexus = avatar === 'nexus' || avatar === 'matrix';
        const isRed = avatar === 'red' || avatar === 'crimson';
        const isArxLogos = avatar === 'arx-logos';
        const isHalcy = !isArxLimes && !isNexus && !isRed && !isArxLogos;

        if (this.particleSystem) this.particleSystem.visible = isHalcy;
        if (this.halcyOuterRing) this.halcyOuterRing.visible = isHalcy;
        if (this.halcyInnerRingGroup) this.halcyInnerRingGroup.visible = isHalcy;
        if (this.coreOrb) this.coreOrb.visible = isHalcy;

        if (this.arxLimesGroup) this.arxLimesGroup.visible = isArxLimes;
        if (this.nexusGroup) this.nexusGroup.visible = isNexus;
        if (this.redGroup) this.redGroup.visible = isRed;
        if (this.arxLogosGroup) this.arxLogosGroup.visible = isArxLogos;
    }

    // Which color palette tints the currently active (and future) avatar shapes.
    setColorTheme(theme) {
        this.currentColorTheme = theme;
        this.applyColorPalette();
    }

    applyColorPalette() {
        const p = THEME_PALETTES[this.currentColorTheme] || THEME_PALETTES.halcy;
        this.activePalette = p;

        // hAlcy particle lattice (the obsidian core + both rings have fixed colors by design,
        // independent of the color theme — see buildHalcyAvatar)
        if (this.particleSystem) {
            const colors = this.particleSystem.geometry.attributes.color.array;
            for (let i = 0; i < this.particleCount; i++) {
                colors[i * 3] = p.r;
                colors[i * 3 + 1] = Math.min(1, p.g + Math.random() * 0.15);
                colors[i * 3 + 2] = p.b;
            }
            this.particleSystem.geometry.attributes.color.needsUpdate = true;
        }

        // Nexus glow halo (particle stream colors are recomputed per-frame from activePalette)
        if (this.nexusSingularityGlow) this.nexusSingularityGlow.material.color.setHex(p.hex);

        // A.R.X.LIMES voxel cubes
        if (this.arxLimesCore) this.arxLimesCore.material.color.setHex(p.hex);
        if (this.arxLimesCrystalMat) this.arxLimesCrystalMat.color.setHex(p.hex2);
        if (this.arxLimesCubeMat) this.arxLimesCubeMat.color.setHex(p.hex);

        // A.R.X.LOGOS jagged star
        if (this.arxLogosSpikeMat) this.arxLogosSpikeMat.color.setHex(p.hex);
        if (this.arxLogosInnerCore) this.arxLogosInnerCore.material.color.setHex(p.hex3);
        if (this.arxLogosRings[0]) this.arxLogosRings[0].material.color.setHex(p.hex);
        if (this.arxLogosRings[1]) this.arxLogosRings[1].material.color.setHex(p.hex2);

        // R.E.D. 9000
        if (this.redCoreSphere) this.redCoreSphere.material.color.setHex(p.hex);
        if (this.redLensOuter) this.redLensOuter.material.color.setHex(p.hex);
        if (this.redBlueCircle) this.redBlueCircle.material.color.setHex(p.hex2);
        if (this.redCyanCircle) this.redCyanCircle.material.color.setHex(p.hex3);
    }

    setState(newState) {
        this.state = newState;
    }

    updateAudioData(dataArray) {
        this.audioData = dataArray;
    }

    animate() {
        requestAnimationFrame(() => this.animate());

        const elapsedTime = this.clock.getElapsedTime();

        let audioSum = 0;
        for (let i = 0; i < 16; i++) {
            audioSum += this.audioData[i] || 0;
        }
        const audioIntensity = audioSum / (16 * 255);

        if (this.currentAvatar === 'arx-logos') {
            // ==============================================================
            // A.R.X.LOGOS: JAGGED GEOMETRIC STAR WITH NEON SPIKES
            // ==============================================================
            if (this.arxLogosGroup) {
                this.arxLogosGroup.rotation.y += 0.007 + (this.state === 'THINKING' ? 0.03 : 0);
                this.arxLogosGroup.rotation.x = Math.sin(elapsedTime * 0.5) * 0.12 + this.mouseY;
                this.arxLogosGroup.rotation.z = Math.cos(elapsedTime * 0.4) * 0.06 + this.mouseX;
            }

            if (this.arxLogosInnerCore) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 1.4;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 18) * 0.4;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.12;
                }
                this.arxLogosInnerCore.scale.set(coreScale, coreScale, coreScale);
                this.arxLogosInnerCore.rotation.y += 0.025;
                this.arxLogosInnerCore.rotation.x += 0.015;
            }

            // Animate each spike — jagged pulsing outward
            this.arxLogosStarPoints.forEach((spike, idx) => {
                let pulseFactor = 1.0;
                if (this.state === 'SPEAKING') {
                    const fVal = (this.audioData[idx % 16] || 0) / 255;
                    pulseFactor = 1.0 + fVal * 0.7 + Math.sin(elapsedTime * 10 + spike.userData.phase) * 0.2;
                } else if (this.state === 'THINKING') {
                    pulseFactor = 1.0 + Math.sin(elapsedTime * 14 + spike.userData.phase) * 0.45;
                } else {
                    pulseFactor = 1.0 + Math.sin(elapsedTime * 3 + spike.userData.phase) * 0.1;
                }
                spike.position.set(
                    spike.userData.baseX * pulseFactor,
                    spike.userData.baseY * pulseFactor,
                    spike.userData.baseZ * pulseFactor
                );
            });

            // Orbit rings
            const sRingSpeed = this.state === 'THINKING' ? 3.5 : (this.state === 'SPEAKING' ? 1.8 : 1.0);
            this.arxLogosRings.forEach(ring => {
                ring.rotation.z += ring.userData.speed * sRingSpeed;
            });

        } else if (this.currentAvatar === 'red' || this.currentAvatar === 'crimson') {
            // ==============================================================
            // R.E.D. 9000: CENTRAL SPHERE + TWO ORBIT CIRCLES
            // ==============================================================
            if (this.redGroup) {
                this.redGroup.rotation.y = Math.sin(elapsedTime * 0.3) * 0.15 + this.mouseY;
                this.redGroup.rotation.x = Math.sin(elapsedTime * 0.2) * 0.1 + this.mouseX;
            }

            // Central Sphere Pulse with Audio / State
            if (this.redCoreSphere) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 1.3;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.3;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
                }
                this.redCoreSphere.scale.set(coreScale, coreScale, coreScale);
            }

            if (this.redLensOuter) {
                this.redLensOuter.rotation.y += 0.008;
                this.redLensOuter.rotation.x += 0.005;
                const lensScale = 1.0 + audioIntensity * 0.6;
                this.redLensOuter.scale.set(lensScale, lensScale, lensScale);
            }

            if (this.redPupil) {
                const pupilScale = 1.0 + audioIntensity * 1.8;
                this.redPupil.scale.set(pupilScale, pupilScale, pupilScale);
            }

            // Rotating Orbit Circles
            const ringSpeedMult = this.state === 'THINKING' ? 3.8 : (this.state === 'SPEAKING' ? 1.8 : 1.0);

            if (this.redBlueCircle) {
                this.redBlueCircle.rotation.z += this.redBlueCircle.userData.speed * ringSpeedMult;
                this.redBlueCircle.rotation.x = 1.1 + Math.sin(elapsedTime * 0.8) * 0.1;
            }

            if (this.redCyanCircle) {
                this.redCyanCircle.rotation.z += this.redCyanCircle.userData.speed * ringSpeedMult;
                this.redCyanCircle.rotation.y = 0.9 + Math.cos(elapsedTime * 0.8) * 0.1;
            }

        } else if (this.currentAvatar === 'nexus' || this.currentAvatar === 'matrix') {
            // ==============================================================
            // THE NEXUS: INWARD FALLING PARTICLES & GRAVITATIONAL SINGULARITY
            // ==============================================================
            if (this.nexusGroup) {
                this.nexusGroup.rotation.y += 0.005 + (this.state === 'THINKING' ? 0.02 : 0);
                this.nexusGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.1 + this.mouseY;
                this.nexusGroup.rotation.z = this.mouseX;
            }

            if (this.nexusSingularity) {
                let singScale = 1.0;
                if (this.state === 'SPEAKING') {
                    singScale = 1.0 + audioIntensity * 2.0;
                } else if (this.state === 'THINKING') {
                    singScale = 1.0 + Math.sin(elapsedTime * 20) * 0.5;
                } else {
                    singScale = 1.0 + Math.sin(elapsedTime * 4) * 0.15;
                }
                this.nexusSingularity.scale.set(singScale, singScale, singScale);
            }

            if (this.nexusSingularityGlow) {
                let glowScale = 1.0 + audioIntensity * 1.5 + Math.sin(elapsedTime * 3) * 0.1;
                this.nexusSingularityGlow.scale.set(glowScale, glowScale, glowScale);
                this.nexusSingularityGlow.rotation.y -= 0.03;
                this.nexusSingularityGlow.rotation.x += 0.02;
            }

            if (this.nexusParticles) {
                const positions = this.nexusParticles.geometry.attributes.position.array;
                const colors = this.nexusParticles.geometry.attributes.color.array;
                const pal = this.activePalette || THEME_PALETTES.halcy;

                const speedMult = this.state === 'THINKING' ? 2.5 : (this.state === 'SPEAKING' ? 1.5 : 1.0);

                for (let i = 0; i < this.nexusParticleCount; i++) {
                    const data = this.nexusData[i];

                    data.angle += data.spiralSpeed * speedMult;
                    data.radius -= data.inwardSpeed * speedMult;
                    data.y -= data.fallSpeed * speedMult;
                    data.y *= 0.985;

                    if (data.radius <= 6 || Math.abs(data.y) > 130) {
                        data.radius = 120 + Math.random() * 50;
                        data.angle = Math.random() * Math.PI * 2;
                        data.y = (Math.random() - 0.5) * 160;
                    }

                    const x = data.radius * Math.cos(data.angle);
                    const z = data.radius * Math.sin(data.angle);

                    positions[i * 3] = x;
                    positions[i * 3 + 1] = data.y;
                    positions[i * 3 + 2] = z;

                    const brightness = Math.min(1.0, 1.2 - (data.radius / 150));
                    colors[i * 3] = pal.r * brightness;
                    colors[i * 3 + 1] = pal.g * brightness;
                    colors[i * 3 + 2] = pal.b * brightness;
                }

                this.nexusParticles.geometry.attributes.position.needsUpdate = true;
                this.nexusParticles.geometry.attributes.color.needsUpdate = true;
            }

        } else if (this.currentAvatar === 'arx-limes') {
            // ==========================================
            // A.R.X.LIMES (VOXEL CUBES)
            // ==========================================
            if (this.arxLimesGroup) {
                let groupRotSpeed = 0.006;
                if (this.state === 'THINKING') groupRotSpeed = 0.035;
                if (this.state === 'SPEAKING') groupRotSpeed = 0.015;

                this.arxLimesGroup.rotation.y += groupRotSpeed;
                this.arxLimesGroup.rotation.x = Math.sin(elapsedTime * 0.5) * 0.15 + this.mouseY;
                this.arxLimesGroup.rotation.z = this.mouseX;
            }

            if (this.arxLimesCore) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 1.2;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 18) * 0.4;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.15;
                }
                this.arxLimesCore.scale.set(coreScale, coreScale, coreScale);
                this.arxLimesCore.rotation.x += 0.02;
                this.arxLimesCore.rotation.y += 0.03;
            }

            this.arxLimesCubes.forEach((cube, idx) => {
                cube.rotation.x += cube.userData.rotSpeedX;
                cube.rotation.y += cube.userData.rotSpeedY;

                let expansion = 1.0;
                if (this.state === 'SPEAKING') {
                    const fVal = (this.audioData[idx % 16] || 0) / 255;
                    expansion = 1.0 + fVal * 0.5 + Math.sin(elapsedTime * 8 + idx) * 0.15;
                } else if (this.state === 'THINKING') {
                    expansion = 1.0 + Math.sin(elapsedTime * 12 + idx * 0.3) * 0.35;
                } else {
                    expansion = 1.0 + Math.sin(elapsedTime * 2 + idx * 0.2) * 0.08;
                }

                cube.position.x = cube.userData.baseX * expansion;
                cube.position.y = cube.userData.baseY * expansion;
                cube.position.z = cube.userData.baseZ * expansion;
            });

        } else {
            // ==========================================
            // HALCY / DEFAULT PARTICLE ANIMATIONS
            // ==========================================
            if (this.particleSystem) {
                const positions = this.particleSystem.geometry.attributes.position.array;

                for (let i = 0; i < this.particleCount; i++) {
                    const base = this.basePositions[i];
                    let displacement = 0;

                    if (this.state === 'SPEAKING') {
                        const freqIdx = i % 32;
                        const freqVal = (this.audioData[freqIdx] || 0) / 255;
                        displacement = Math.sin(elapsedTime * 8 + i * 0.1) * (8 + freqVal * 25);
                    } else if (this.state === 'LISTENING') {
                        displacement = Math.sin(elapsedTime * 6 - Math.sqrt(base.x**2 + base.y**2 + base.z**2) * 0.1) * 6;
                    } else if (this.state === 'THINKING') {
                        displacement = Math.sin(elapsedTime * 12 + base.x * 0.2) * Math.cos(elapsedTime * 8 + base.y * 0.2) * 9;
                    } else {
                        displacement = Math.sin(elapsedTime * 2 + base.y * 0.05) * 3;
                    }

                    const scale = 1 + displacement / this.halcyLatticeRadius;
                    positions[i * 3] = base.x * scale;
                    positions[i * 3 + 1] = base.y * scale;
                    positions[i * 3 + 2] = base.z * scale;
                }

                this.particleSystem.geometry.attributes.position.needsUpdate = true;

                let rotSpeed = 0.004;
                if (this.state === 'THINKING') rotSpeed = 0.025;
                if (this.state === 'SPEAKING') rotSpeed = 0.01;

                this.particleSystem.rotation.y += rotSpeed;
                this.particleSystem.rotation.x = Math.sin(elapsedTime * 0.5) * 0.1 + this.mouseY;
                this.particleSystem.rotation.z = this.mouseX;
            }

            // Static outer ring — gentle idle rotation, fixed cyan
            if (this.halcyOuterRing) {
                const speedMultiplier = this.state === 'THINKING' ? 3.5 : (this.state === 'SPEAKING' ? 1.8 : 1.0);
                this.halcyOuterRing.rotation.z += this.halcyOuterRing.userData.speed * speedMultiplier;
                this.halcyOuterRing.rotation.x = this.halcyOuterRing.userData.baseRotX + Math.sin(elapsedTime * 0.8) * 0.08 + this.mouseY;
                this.halcyOuterRing.rotation.y = this.halcyOuterRing.userData.baseRotY + Math.cos(elapsedTime * 0.8) * 0.08 + this.mouseX;
            }

            // Inner ultramarine equalizer ring — each segment thickens along the circumference
            // to the live audio frequencies while speaking. The segments are children of the
            // rotating group, so the thickening pattern rotates together with the ring itself.
            this.halcyInnerSegments.forEach((seg, idx) => {
                let lenScale = 1.0;
                if (this.state === 'SPEAKING') {
                    const fVal = (this.audioData[idx % 32] || 0) / 255;
                    lenScale = 1.0 + fVal * 2.4;
                } else if (this.state === 'THINKING') {
                    lenScale = 1.0 + Math.sin(elapsedTime * 14 + seg.userData.angle * 6) * 0.35;
                } else if (this.state === 'LISTENING') {
                    lenScale = 1.0 + Math.sin(elapsedTime * 6 + seg.userData.angle * 4) * 0.15;
                } else {
                    lenScale = 1.0 + Math.sin(elapsedTime * 2 + seg.userData.angle * 3) * 0.08;
                }
                seg.scale.y = lenScale;
            });

            if (this.halcyInnerRingGroup) {
                const spinMultiplier = this.state === 'THINKING' ? 3.0 : (this.state === 'SPEAKING' ? 1.6 : 1.0);
                this.halcyInnerRingGroup.rotation.z += this.halcyInnerRingGroup.userData.speed * spinMultiplier;

                const swayAmplitude = this.state === 'SPEAKING' ? 0.24 : 0.05;
                const swaySpeed = this.state === 'SPEAKING' ? 2.4 : 0.6;
                this.halcyInnerRingGroup.rotation.x = Math.sin(elapsedTime * swaySpeed) * swayAmplitude;
            }

            if (this.coreOrb) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 0.5;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 15) * 0.2;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
                }
                this.coreOrb.scale.set(coreScale, coreScale, coreScale);
                this.coreOrb.rotation.y -= 0.02;
            }
        }

        this.renderer.render(this.scene, this.camera);
    }

    onResize() {
        if (!this.container || !this.renderer || !this.camera) return;
        const width = this.container.clientWidth;
        const height = this.container.clientHeight;
        this.camera.aspect = width / height;
        this.camera.updateProjectionMatrix();
        this.renderer.setSize(width, height);
    }
}

window.HologramAvatar = HologramAvatar;
