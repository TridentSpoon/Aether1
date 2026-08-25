/**
 * 3D Holographic Avatar Engine (Three.js)
 * Supports:
 * 1. UNSC Cortana Harmonic Particle Lattice & Orbital Rings (Cyan/Blue)
 * 2. Cephalon Simaris Floating 3D Voxel Hypercubes & Crystalline Monolith (Warframe Amber)
 * 3. The Nexus: Inward Falling Digital Letters into a Gravitational Singularity (Matrix Green)
 * 4. R.E.D. 9000 (HAL 9000): Central Glowing Red Sphere with One Blue Circle and One Cyan Circle
 * 5. Cephalon Suda: Jagged Geometric Star with Magenta Particle Burst
 * 6. Real-time Audio Frequency and State deformation.
 */

class HologramAvatar {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.state = 'IDLE'; // IDLE, LISTENING, THINKING, SPEAKING
        this.currentTheme = 'cortana'; // cortana, simaris, nexus, red/crimson, suda
        this.audioData = new Uint8Array(64);
        
        this.scene = null;
        this.camera = null;
        this.renderer = null;
        
        // 1. Cortana structures
        this.particleSystem = null;
        this.particleCount = 2800;
        this.basePositions = [];
        this.rings = [];
        this.coreOrb = null;

        // 2. Cephalon Simaris structures
        this.simarisGroup = null;
        this.simarisCubes = [];
        this.simarisCore = null;

        // 3. The Nexus Matrix Singularity structures
        this.nexusGroup = null;
        this.nexusParticles = null;
        this.nexusParticleCount = 1400;
        this.nexusData = [];
        this.nexusSingularity = null;
        this.nexusSingularityGlow = null;

        // 4. R.E.D. 9000 / HAL 9000 structures (Central Red Sphere + Blue Circle + Cyan Circle)
        this.redGroup = null;
        this.redCoreSphere = null;
        this.redLensOuter = null;
        this.redPupil = null;
        this.redBlueCircle = null;
        this.redCyanCircle = null;

        // 5. Cephalon Suda - Jagged Geometric Star
        this.sudaGroup = null;
        this.sudaStarPoints = [];
        this.sudaInnerCore = null;
        this.sudaRings = [];
        
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

        this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
        this.renderer.setSize(width, height);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.container.innerHTML = '';
        this.container.appendChild(this.renderer.domElement);

        this.clock = new THREE.Clock();

        // Build all avatar architectures
        this.buildCortanaAvatar();
        this.buildSimarisAvatar();
        this.buildNexusAvatar();
        this.buildRed9000Avatar();
        this.buildSudaAvatar();

        // Initial theme setup
        this.setTheme(this.currentTheme);

        window.addEventListener('mousemove', (e) => {
            this.mouseX = (e.clientX - window.innerWidth / 2) * 0.0005;
            this.mouseY = (e.clientY - window.innerHeight / 2) * 0.0005;
        });

        window.addEventListener('resize', () => this.onResize());

        this.animate();
    }

    buildCortanaAvatar() {
        const geometry = new THREE.BufferGeometry();
        const positions = new Float32Array(this.particleCount * 3);
        const colors = new Float32Array(this.particleCount * 3);
        const radius = 60;

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

        const canvas = document.createElement('canvas');
        canvas.width = 32;
        canvas.height = 32;
        const ctx = canvas.getContext('2d');
        const gradient = ctx.createRadialGradient(16, 16, 0, 16, 16, 16);
        gradient.addColorStop(0, 'rgba(255,255,255,1)');
        gradient.addColorStop(0.3, 'rgba(0,240,255,0.8)');
        gradient.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = gradient;
        ctx.fillRect(0, 0, 32, 32);

        const texture = new THREE.CanvasTexture(canvas);

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

        const ringConfigs = [
            { radius: 82, width: 1.5, rotX: 0.6, rotY: 0.2, color: 0x00f0ff, speed: 0.015 },
            { radius: 92, width: 1.0, rotX: -0.4, rotY: 0.8, color: 0x00a2ff, speed: -0.012 },
            { radius: 104, width: 0.8, rotX: 1.2, rotY: -0.5, color: 0xb026ff, speed: 0.008 }
        ];

        ringConfigs.forEach(cfg => {
            const geom = new THREE.RingGeometry(cfg.radius - cfg.width, cfg.radius, 64);
            const mat = new THREE.MeshBasicMaterial({
                color: cfg.color,
                side: THREE.DoubleSide,
                transparent: true,
                opacity: 0.45,
                blending: THREE.AdditiveBlending
            });
            const ring = new THREE.Mesh(geom, mat);
            ring.rotation.x = cfg.rotX;
            ring.rotation.y = cfg.rotY;
            ring.userData = { speed: cfg.speed, baseRotX: cfg.rotX, baseRotY: cfg.rotY };
            this.rings.push(ring);
            this.scene.add(ring);
        });

        const coreGeom = new THREE.SphereGeometry(18, 32, 32);
        const coreMat = new THREE.MeshBasicMaterial({
            color: 0x00f0ff,
            transparent: true,
            opacity: 0.35,
            wireframe: true,
            blending: THREE.AdditiveBlending
        });
        this.coreOrb = new THREE.Mesh(coreGeom, coreMat);
        this.scene.add(this.coreOrb);
    }

    buildSimarisAvatar() {
        this.simarisGroup = new THREE.Group();
        
        const coreGeom = new THREE.BoxGeometry(28, 28, 28);
        const coreMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00,
            wireframe: true,
            transparent: true,
            opacity: 0.8,
            blending: THREE.AdditiveBlending
        });
        this.simarisCore = new THREE.Mesh(coreGeom, coreMat);
        this.simarisGroup.add(this.simarisCore);

        const crystalGeom = new THREE.OctahedronGeometry(14, 0);
        const crystalMat = new THREE.MeshBasicMaterial({
            color: 0xff5500,
            transparent: true,
            opacity: 0.9,
            blending: THREE.AdditiveBlending
        });
        const crystal = new THREE.Mesh(crystalGeom, crystalMat);
        this.simarisCore.add(crystal);

        const cubeGeom = new THREE.BoxGeometry(8, 8, 8);
        const cubeMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00,
            wireframe: true,
            transparent: true,
            opacity: 0.65,
            blending: THREE.AdditiveBlending
        });

        const numCubes = 54;
        for (let i = 0; i < numCubes; i++) {
            const cube = new THREE.Mesh(cubeGeom, cubeMat);
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
            this.simarisCubes.push(cube);
            this.simarisGroup.add(cube);
        }

        this.scene.add(this.simarisGroup);
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

        const canvas = document.createElement('canvas');
        canvas.width = 64;
        canvas.height = 64;
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = '#000000';
        ctx.fillRect(0, 0, 64, 64);
        ctx.font = 'bold 36px monospace';
        ctx.fillStyle = '#00ff66';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.shadowColor = '#00ffaa';
        ctx.shadowBlur = 10;
        ctx.fillText('0', 32, 32);

        const matrixTexture = new THREE.CanvasTexture(canvas);

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

        // 1. Central Glowing Red Sphere (HAL / Reactive Daemon Eye)
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

        // 2. One Electric Blue Circle (Ring 1)
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

        // 3. One Neon Cyan Circle (Ring 2)
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

    buildSudaAvatar() {
        this.sudaGroup = new THREE.Group();

        // Jagged Geometric Star — built from sharp spike tetrahedra radiating from center
        const spikeMat = new THREE.MeshBasicMaterial({
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
            const spike = new THREE.Mesh(coneGeom, spikeMat);

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

            this.sudaStarPoints.push(spike);
            this.sudaGroup.add(spike);
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
        this.sudaInnerCore = new THREE.Mesh(coreGeom, coreMat);
        this.sudaGroup.add(this.sudaInnerCore);

        // Two thin magenta orbit rings around the star
        const ring1Geom = new THREE.RingGeometry(90, 92, 48);
        const ring1Mat = new THREE.MeshBasicMaterial({
            color: 0xe024c3,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.55,
            blending: THREE.AdditiveBlending
        });
        const sudaRing1 = new THREE.Mesh(ring1Geom, ring1Mat);
        sudaRing1.rotation.x = 0.5;
        sudaRing1.userData = { speed: 0.022 };
        this.sudaRings.push(sudaRing1);
        this.sudaGroup.add(sudaRing1);

        const ring2Geom = new THREE.RingGeometry(110, 112, 48);
        const ring2Mat = new THREE.MeshBasicMaterial({
            color: 0x9d00ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.45,
            blending: THREE.AdditiveBlending
        });
        const sudaRing2 = new THREE.Mesh(ring2Geom, ring2Mat);
        sudaRing2.rotation.x = -0.9;
        sudaRing2.rotation.y = 0.6;
        sudaRing2.userData = { speed: -0.016 };
        this.sudaRings.push(sudaRing2);
        this.sudaGroup.add(sudaRing2);

        this.scene.add(this.sudaGroup);
    }

    setTheme(theme) {
        this.currentTheme = theme;
        const isSimaris = theme === 'simaris';
        const isNexus = theme === 'nexus' || theme === 'matrix';
        const isRed = theme === 'red' || theme === 'crimson';
        const isSuda = theme === 'suda';
        const isCortana = !isSimaris && !isNexus && !isRed && !isSuda;

        // Toggle visibility
        if (this.particleSystem) this.particleSystem.visible = isCortana;
        this.rings.forEach(r => r.visible = isCortana);
        if (this.coreOrb) this.coreOrb.visible = isCortana;

        if (this.simarisGroup) this.simarisGroup.visible = isSimaris;
        if (this.nexusGroup) this.nexusGroup.visible = isNexus;
        if (this.redGroup) this.redGroup.visible = isRed;
        if (this.sudaGroup) this.sudaGroup.visible = isSuda;
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

        if (this.currentTheme === 'suda') {
            // ==============================================================
            // CEPHALON SUDA: JAGGED GEOMETRIC STAR WITH MAGENTA NEON SPIKES
            // ==============================================================
            if (this.sudaGroup) {
                this.sudaGroup.rotation.y += 0.007 + (this.state === 'THINKING' ? 0.03 : 0);
                this.sudaGroup.rotation.x = Math.sin(elapsedTime * 0.5) * 0.12 + this.mouseY;
                this.sudaGroup.rotation.z = Math.cos(elapsedTime * 0.4) * 0.06 + this.mouseX;
            }

            if (this.sudaInnerCore) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 1.4;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 18) * 0.4;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.12;
                }
                this.sudaInnerCore.scale.set(coreScale, coreScale, coreScale);
                this.sudaInnerCore.rotation.y += 0.025;
                this.sudaInnerCore.rotation.x += 0.015;
            }

            // Animate each spike — jagged pulsing outward
            this.sudaStarPoints.forEach((spike, idx) => {
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
            this.sudaRings.forEach(ring => {
                ring.rotation.z += ring.userData.speed * sRingSpeed;
            });

        } else if (this.currentTheme === 'red' || this.currentTheme === 'crimson') {
            // ==============================================================
            // R.E.D. 9000: CENTRAL RED SPHERE + BLUE CIRCLE + CYAN CIRCLE
            // ==============================================================
            if (this.redGroup) {
                this.redGroup.rotation.y = Math.sin(elapsedTime * 0.3) * 0.15 + this.mouseY;
                this.redGroup.rotation.x = Math.sin(elapsedTime * 0.2) * 0.1 + this.mouseX;
            }

            // Central Red Sphere Pulse with Audio / State
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

            // Rotating Blue and Cyan Circles
            const ringSpeedMult = this.state === 'THINKING' ? 3.8 : (this.state === 'SPEAKING' ? 1.8 : 1.0);

            if (this.redBlueCircle) {
                this.redBlueCircle.rotation.z += this.redBlueCircle.userData.speed * ringSpeedMult;
                this.redBlueCircle.rotation.x = 1.1 + Math.sin(elapsedTime * 0.8) * 0.1;
            }

            if (this.redCyanCircle) {
                this.redCyanCircle.rotation.z += this.redCyanCircle.userData.speed * ringSpeedMult;
                this.redCyanCircle.rotation.y = 0.9 + Math.cos(elapsedTime * 0.8) * 0.1;
            }

        } else if (this.currentTheme === 'nexus' || this.currentTheme === 'matrix') {
            // ==============================================================
            // THE NEXUS: INWARD FALLING LETTERS & GRAVITATIONAL SINGULARITY
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
                    colors[i * 3] = 0.1 * brightness;
                    colors[i * 3 + 1] = brightness;
                    colors[i * 3 + 2] = 0.4 * brightness;
                }

                this.nexusParticles.geometry.attributes.position.needsUpdate = true;
                this.nexusParticles.geometry.attributes.color.needsUpdate = true;
            }

        } else if (this.currentTheme === 'simaris') {
            // ==========================================
            // CEPHALON SIMARIS (WARFRAME VOXEL CUBES)
            // ==========================================
            if (this.simarisGroup) {
                let groupRotSpeed = 0.006;
                if (this.state === 'THINKING') groupRotSpeed = 0.035;
                if (this.state === 'SPEAKING') groupRotSpeed = 0.015;

                this.simarisGroup.rotation.y += groupRotSpeed;
                this.simarisGroup.rotation.x = Math.sin(elapsedTime * 0.5) * 0.15 + this.mouseY;
                this.simarisGroup.rotation.z = this.mouseX;
            }

            if (this.simarisCore) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 1.2;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 18) * 0.4;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.15;
                }
                this.simarisCore.scale.set(coreScale, coreScale, coreScale);
                this.simarisCore.rotation.x += 0.02;
                this.simarisCore.rotation.y += 0.03;
            }

            this.simarisCubes.forEach((cube, idx) => {
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
            // CORTANA / DEFAULT PARTICLE ANIMATIONS
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

                    const scale = 1 + displacement / 60;
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

            this.rings.forEach((ring, idx) => {
                const speedMultiplier = this.state === 'THINKING' ? 3.5 : (this.state === 'SPEAKING' ? 1.8 : 1.0);
                ring.rotation.z += ring.userData.speed * speedMultiplier;
                ring.rotation.x = ring.userData.baseRotX + Math.sin(elapsedTime * 0.8 + idx) * 0.08 + this.mouseY;
                ring.rotation.y = ring.userData.baseRotY + Math.cos(elapsedTime * 0.8 + idx) * 0.08 + this.mouseX;
            });

            if (this.coreOrb) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 0.8;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 15) * 0.35;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.12;
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
