/**
 * 3D Holographic Avatar Engine (Three.js)
 * Avatars (3D shape) and Color Themes (palette) are independently selectable.
 * Avatars:
 * 1. hAlcy: Harmonic Particle Lattice & Orbital Rings
 * 2. A.R.X.LIMES: Faceted Floating Hub with a Fractured Convex Dome of Plates
 * 3. The Nexus: Squid/Brain Creature with Trailing Tentacles that Hunts the Cursor,
 *    Set Against Falling NEXUS Letter Rain
 * 4. R.E.D. 9000 (HAL 9000): Obsidian Eye with Two Static Eyelid Arcs
 * 5. A.R.X.LOGOS: Central Hexagon with Six Clockwise Spiraling Hexagon Arms & Dotted Hex Frame
 * 6. Real-time Audio Frequency and State deformation.
 *
 * This file defines the class shell (construction, lifecycle, avatar/theme selection).
 * Geometry construction lives in avatar-*.js, shared geometry helpers in geometry-helpers.js,
 * and the per-frame animation loop in animate.js -- all attached via HologramAvatar.prototype,
 * so load order in index.html matters: this file first, then the rest, before app.js runs.
 */

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

        // 2. A.R.X.LIMES structures — floating faceted hub + fractured dome plates
        this.arxLimesGroup = null;
        this.arxLimesHubMesh = null;
        this.arxLimesHubOutline = null;
        this.arxLimesPlates = []; // { group, baseAngle, baseRadius, phase, tier }
        this.arxLimesHubFillMat = null;
        this.arxLimesOutlineMat = null;
        this.arxLimesMainFillMat = null;
        this.arxLimesWingFillMat = null;

        // 3. The Nexus: squid/brain hunting the cursor, trailing tentacles, NEXUS letter rain
        this.nexusGroup = null;
        this.nexusCreatureGroup = null; // head + tentacles; rotates to face the cursor
        this.nexusHeadMesh = null;
        this.nexusHeadOutline = null;
        this.nexusTentacleMat = null;
        this.nexusTentacles = []; // { segments, baseAngle, spreadRadius }
        this.nexusRainDrops = []; // sprites, fall straight down and wrap top-to-bottom
        this.nexusRainMaterials = []; // one shared material per NEXUS letter
        this.nexusFacing = { yaw: 0, pitch: 0 }; // eased hunting orientation

        // 4. R.E.D. 9000 / HAL 9000 structures -- obsidian eye with lens shell + eyelid arcs
        this.redGroup = null;
        this.redCoreSphere = null;
        this.redLensOuter = null;
        this.redBlueCircle = null;
        this.redCyanCircle = null;

        // 5. A.R.X.LOGOS - Central Hexagon with Six Spiraling Hexagon Arms + Dotted Outer Ring
        this.arxLogosGroup = null;
        this.arxLogosCentralFill = null;
        this.arxLogosCentralOutline = null;
        this.arxLogosArmHexes = []; // { mesh, armIndex, stepIndex }
        this.arxLogosOuterDots = [];
        this.arxLogosArmMatNear = null;
        this.arxLogosArmMatMid = null;
        this.arxLogosArmMatFar = null;
        this.arxLogosOuterDotMat = null;

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

        // Window resize always resizes the container, but the container can also change size
        // on its own (layout/panel changes) without a window resize event -- ResizeObserver
        // catches both; the window listener stays as a fallback where ResizeObserver is unavailable.
        if (typeof ResizeObserver !== 'undefined') {
            this.resizeObserver = new ResizeObserver(() => this.onResize());
            this.resizeObserver.observe(this.container);
        } else {
            window.addEventListener('resize', () => this.onResize());
        }

        this.animate();
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

        // The Nexus: squid/brain creature + letter rain
        if (this.nexusHeadMesh) this.nexusHeadMesh.material.color.setHex(p.hex);
        if (this.nexusHeadOutline) this.nexusHeadOutline.material.color.setHex(p.hex3);
        if (this.nexusTentacleMat) this.nexusTentacleMat.color.setHex(p.hex2);
        this.nexusRainMaterials.forEach(mat => mat.color.setHex(p.hex));

        // A.R.X.LIMES fractured dome
        if (this.arxLimesHubFillMat) this.arxLimesHubFillMat.color.setHex(p.hex);
        if (this.arxLimesMainFillMat) this.arxLimesMainFillMat.color.setHex(p.hex);
        if (this.arxLimesWingFillMat) this.arxLimesWingFillMat.color.setHex(p.hex2);
        if (this.arxLimesOutlineMat) this.arxLimesOutlineMat.color.setHex(p.hex3);

        // A.R.X.LOGOS hexagon spiral
        if (this.arxLogosCentralFill) this.arxLogosCentralFill.material.color.setHex(p.hex);
        if (this.arxLogosCentralOutline) this.arxLogosCentralOutline.material.color.setHex(p.hex);
        if (this.arxLogosArmMatNear) this.arxLogosArmMatNear.color.setHex(p.hex);
        if (this.arxLogosArmMatMid) this.arxLogosArmMatMid.color.setHex(p.hex2);
        if (this.arxLogosArmMatFar) this.arxLogosArmMatFar.color.setHex(p.hex3);
        if (this.arxLogosOuterDotMat) this.arxLogosOuterDotMat.color.setHex(p.hex3);

        // R.E.D. 9000 (the obsidian core is a fixed material, not theme-tinted -- see buildRed9000Avatar)
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
