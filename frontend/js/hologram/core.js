/**
 * 3D Holographic Avatar Engine (Three.js)
 * Avatars (3D shape) and Color Themes (palette) are independently selectable.
 * Avatars:
 * 1. hAlcy: Harmonic Particle Lattice & Orbital Rings
 * 2. A.R.X.LIMES: Faceted Floating Hub with a Fractured Convex Dome of Plates
 * 3. The Nexus: Squid/Brain Creature with Trailing Tentacles that Hunts the Cursor,
 *    Set Against Falling NEXUS Letter Rain
 * 4. R.E.D. 9000 (HAL 9000): Obsidian Eye with a Hot Lens Glow, Two Depth-Stacked Blinking
 *    Eyelid Arcs, and a Speech-Reactive Light
 * 5. A.R.X.LOGOS: Hexagonal Core Eye with Six Spiraling Aperture-Blade Arms, Two Tumbling
 *    Hex-Frame Rings, and an Orbiting Hex Swarm
 * 6. A1ter_nul: Cunningham -- a Faceted Chromatic-Glitch Ghost Bust Behind a Rotating Firewall/ICE Ring
 * 7. Real-time Audio Frequency and State deformation.
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
        this.currentAvatar = 'halcy'; // halcy, arx-limes, nexus, red, arx-logos, alt
        this.currentColorTheme = 'halcy'; // halcy, nexus, arx-limes, arx-logos, red, night-city
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

        // 3. The Nexus Crew (v3): an elongated, bulbous lathe-hull rendered as a wireframe/
        // point-cloud "live sensor feed" (not a solid mesh), the previous build's eye-lens
        // cluster carried over onto the nose, and 10 chain-link tentacles that drift
        // jellyfish-style when idle and spread/sharpen forward toward the cursor when alert
        // -- set against a falling NEXUS letter rain and a faint radar-grid backdrop.
        this.nexusGroup = null;
        this.nexusCreatureGroup = null; // hull + tentacles; rotates to face the cursor
        this.nexusHeadMesh = null; // near-invisible fill (vertex-colored gradient), just enough for the point cloud to sit "on"
        this.nexusHullFillMat = null;
        this.nexusHeadInnerWire = null; // dense inner wireframe (every triangle edge) -- dimmer, "inner structural lines"
        this.nexusHullInnerWireMat = null;
        this.nexusHeadOutline = null; // sparser structural/silhouette edges -- brighter, additive
        this.nexusHeadOutlineMat = null;
        this.nexusHeadPoints = null; // sparse point cloud at every hull vertex -- brightest of the three, the "nodes" depth cue
        this.nexusHullPointsMat = null;
        this.nexusHeadHeightScale = 1; // head-on Y squash (0.6 -- same width, 60% height); animateNexus's pulse multiplies on top of this
        this.nexusRibMat = null; // shared material for the three carapace ribs
        this.nexusGridMat = null; // faint rotating radar-grid backdrop sprite
        this.nexusTentacleLinkMat = null; // shared tapered-cylinder chain-link material
        this.nexusTentaclePointMat = null; // shared glowing joint-marker material, brighter than the links
        this.nexusTentacles = []; // { geom, positions, points, links, claws, baseAngle, speedMult, phaseSeed, aggroT }
        this.nexusEyes = []; // { mesh, glow, ring, highlight } — fixed phosphor red/orange, blink together in idle
        this.nexusEyeRingMat = null; // shared bezel-outline sprite material, slowly spins
        this.nexusEyeHighlightMat = null; // shared anime/cartoon eye-shine sprite material
        this.nexusRainDrops = []; // sprites, fall straight down and wrap top-to-bottom
        this.nexusRainMaterials = []; // one shared material per NEXUS letter
        this.nexusFacing = { yaw: 0, pitch: 0, roll: 0 }; // eased head orientation

        // 4. R.E.D. 9000 / HAL 9000 structures -- obsidian eye with lens shell + eyelid arcs
        this.redGroup = null;
        this.redCoreSphere = null;
        this.redLensOuter = null;
        this.redBlueCircle = null;
        this.redCyanCircle = null;
        // Lens glow: a fixed yellow-hot-to-crimson sprite sitting on the core's front face,
        // plus a warm point light seated at the lens -- both pulse with speech/thinking/
        // blink in animateRed9000. Fixed color regardless of theme, same convention as the
        // obsidian core itself.
        this.redLensGlow = null;
        this.redEyeLight = null;
        // Blink scheduling -- see animateRed9000. redBlinkStartTime is elapsedTime when the
        // eyelids started closing; redNextBlinkTime is when the next autonomous blink fires.
        this.redBlinkStartTime = -999;
        this.redNextBlinkTime = 4 + Math.random() * 4;
        this.redLastHandledBlinkClick = -999; // dedupes a click's blink to a single trigger

        // 5. A.R.X.LOGOS - Central Hexagon with Six Spiraling Hexagon Arms + Dotted Outer Ring
        this.arxLogosGroup = null;
        this.arxLogosCentralFill = null;
        this.arxLogosCentralOutline = null;
        this.arxLogosArmHexes = []; // { mesh, armIndex, stepIndex, baseX, baseY, phase }
        this.arxLogosOuterDots = [];
        this.arxLogosArmMatNear = null;
        this.arxLogosArmMatMid = null;
        this.arxLogosArmMatFar = null;
        this.arxLogosOuterDotMat = null;
        // Core eye -- a fixed dark pupil + catchlight nested in the central hex (real "eye"
        // material, not theme-tinted, same convention as every other avatar's obsidian core).
        this.arxLogosPupil = null;
        this.arxLogosCatchlight = null;
        // Two hex-frame rings beyond the dotted boundary, each tumbling on its own axis --
        // see buildArxLogosAvatar.
        this.arxLogosShellRings = []; // { mesh, speed: {x,y,z} }
        this.arxLogosShellRingMats = [];
        // A small swarm of hex nodes orbiting freely in 3D around the whole structure,
        // unlike the arm hexes (locked to the flat spiral) -- real depth beyond one plane.
        this.arxLogosSwarmGroup = null;
        this.arxLogosSwarmNodes = []; // { mesh, radius, angle, heightOffset, speed, bobPhase }
        this.arxLogosSwarmMat = null;
        // Aperture-close scheduling -- see animateArxLogos. Pulls every arm hex in toward
        // the core-eye and back, like a camera iris snapping shut over it. Same click +
        // autonomous-idle-timer pattern as R.E.D. 9000's blink.
        this.arxLogosApertureStartTime = -999;
        this.arxLogosNextApertureTime = 5 + Math.random() * 4;
        this.arxLogosLastHandledApertureClick = -999;

        // 6. A1ter_nul (Cunningham) -- modelled on Cyberpunk 2077's Black Wall / relic: a
        // stack of irregular dark-glass shards with gaps between them (not a solid body),
        // behind a rotating firewall/ICE perimeter ring. Each shard's glass fill is a
        // fixed obsidian material; its glowing edge is theme-tinted and, while speaking,
        // driven by its own audio frequency bin -- the stack as a vertical equalizer built
        // out of broken relic glass -- see buildAltAvatar.
        this.altGroup = null;
        this.altShardGroup = null;
        this.altShards = []; // { fillMat, outlineMat, baseOpacity, phase, binIndex }
        this.altFirewallRing = null;
        this.altFirewallRingMat = null;
        this.altShieldGroup = null;
        this.altShieldTiles = []; // { fill, outline, baseAngle, phase }
        this.altShieldFillMat = null;
        this.altShieldOutlineMat = null;

        // Avatars registered from outside this engine -- see registerAvatar below and
        // js/hologram/README.md. Each entry is { def, model }: the definition someone
        // handed us, and whatever their build() returned.
        this.plugins = new Map();

        this.clock = null;
        this.lastClickTime = -999; // seconds on this.clock; drives the click-reaction pulse

        // Drag-to-spin -- lets you grab the whole hologram and turn it, like the reference
        // Cortana build's OrbitControls, but scoped to yaw only (no pitch) since none of
        // these avatars are built with a proper backside/underside. Applied to this.scene's
        // own rotation.y, which nothing else here touches, so it composes for free with every
        // avatar's existing local spins and tilts instead of fighting them. A short drag under
        // DRAG_CLICK_THRESHOLD still counts as a click (see the pointerup handler below), so
        // the existing click-reaction pulse keeps working on a tap.
        this.isDraggingView = false;
        this.dragLastX = 0;
        this.dragDistance = 0;
        this.viewSpinVelocity = 0; // radians/frame, decays via damping once released

        // Pointer tracking -- only The Nexus consumes these (mouse-manipulated head that
        // autonomously "hunts" when the pointer isn't actively directing it); other avatars
        // stay front-facing/static and simply don't read them.
        this.nexusMouseNX = 0; // -1..1 across the renderer element
        this.nexusMouseNY = 0;
        this.lastMouseMoveTime = -999; // seconds on this.clock

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
        this.buildAltAvatar();
        this.buildRegisteredAvatars();

        // Initial avatar shape + color theme setup (independent of each other)
        this.setAvatar(this.currentAvatar);
        this.setColorTheme(this.currentColorTheme);

        // Avatars sit front-facing and static at rest; a click wakes them up with a reaction
        // pulse, and a drag spins the whole hologram on its Y axis (with inertia -- see
        // animate.js) so you can turn it to look from another angle.
        const DRAG_CLICK_THRESHOLD = 6; // px; drags shorter than this still count as a click
        const YAW_PER_PIXEL = 0.006; // radians of scene.rotation.y per pixel dragged
        this.renderer.domElement.style.cursor = 'grab';
        this.renderer.domElement.addEventListener('pointerdown', (e) => {
            this.isDraggingView = true;
            this.dragLastX = e.clientX;
            this.dragDistance = 0;
            this.viewSpinVelocity = 0;
            this.renderer.domElement.setPointerCapture(e.pointerId);
            this.renderer.domElement.style.cursor = 'grabbing';
        });
        this.renderer.domElement.addEventListener('pointermove', (e) => {
            if (!this.isDraggingView) return;
            const dx = e.clientX - this.dragLastX;
            this.dragLastX = e.clientX;
            this.dragDistance += Math.abs(dx);
            const delta = dx * YAW_PER_PIXEL;
            this.scene.rotation.y += delta;
            this.viewSpinVelocity = delta; // carried into animate.js as release inertia
        });
        const endDrag = (e) => {
            if (!this.isDraggingView) return;
            this.isDraggingView = false;
            this.renderer.domElement.style.cursor = 'grab';
            if (this.dragDistance < DRAG_CLICK_THRESHOLD) {
                this.viewSpinVelocity = 0; // a tap shouldn't also fling the view
                this.lastClickTime = this.clock.getElapsedTime();
            }
        };
        this.renderer.domElement.addEventListener('pointerup', endDrag);
        this.renderer.domElement.addEventListener('pointercancel', endDrag);

        // The Nexus's head can be steered by the pointer while it's active over the
        // viewport; animateNexus falls back to an autonomous "hunting" scan the moment it
        // stops (mouseleave included, so it doesn't just sit aimed at wherever it was left).
        this.renderer.domElement.addEventListener('mousemove', (e) => {
            const rect = this.renderer.domElement.getBoundingClientRect();
            this.nexusMouseNX = ((e.clientX - rect.left) / rect.width) * 2 - 1;
            this.nexusMouseNY = ((e.clientY - rect.top) / rect.height) * 2 - 1;
            this.lastMouseMoveTime = this.clock.getElapsedTime();
        });
        this.renderer.domElement.addEventListener('mouseleave', () => {
            this.lastMouseMoveTime = -999;
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
        // A registered avatar wins over every built-in, including the hAlcy fallback
        // below -- otherwise an unrecognised name would show hAlcy *and* the plug-in.
        const plugin = this.plugins.get(avatar);
        this.plugins.forEach((entry, id) => {
            if (entry.model && entry.model.group) entry.model.group.visible = (id === avatar);
        });
        const isArxLimes = avatar === 'arx-limes';
        const isNexus = avatar === 'nexus' || avatar === 'matrix';
        const isRed = avatar === 'red' || avatar === 'crimson';
        const isArxLogos = avatar === 'arx-logos';
        const isAlt = avatar === 'alt' || avatar === 'cunningham' || avatar === 'a1ter_nul';
        const isHalcy = !plugin && !isArxLimes && !isNexus && !isRed && !isArxLogos && !isAlt;

        if (this.particleSystem) this.particleSystem.visible = isHalcy;
        if (this.halcyOuterRing) this.halcyOuterRing.visible = isHalcy;
        if (this.halcyInnerRingGroup) this.halcyInnerRingGroup.visible = isHalcy;
        if (this.coreOrb) this.coreOrb.visible = isHalcy;

        if (this.arxLimesGroup) this.arxLimesGroup.visible = isArxLimes;
        if (this.nexusGroup) this.nexusGroup.visible = isNexus;
        if (this.redGroup) this.redGroup.visible = isRed;
        if (this.arxLogosGroup) this.arxLogosGroup.visible = isArxLogos;
        if (this.altGroup) this.altGroup.visible = isAlt;

        // CRT scanline/flicker overlay (see #hologram-viewport.crt-active::after in
        // A1theme.css / sprite.css) -- only The Nexus Crew's wireframe "live sensor feed"
        // look asks for it.
        if (this.container) this.container.classList.toggle('crt-active', isNexus);

        // Chromatic-glitch scanline overlay (see #hologram-viewport.glitch-active::after in
        // A1theme.css / sprite.css) -- only A1ter_nul's digital-ghost look asks for it.
        if (this.container) this.container.classList.toggle('glitch-active', isAlt);
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

        // The Nexus Crew: wireframe hull + chain tentacles + letter rain. The near-invisible
        // fill and dense inner wireframe pick up the base hue; the sparser structural
        // outline, points, and ribs pick up hex3 so they read distinctly brighter -- the
        // "inner lines dimmer than outer edges/nodes" depth cue. The eye-lens cluster and
        // pincer tips are a fixed red regardless of theme (see buildNexusAvatar), like every
        // avatar's always-lit accent.
        if (this.nexusHullFillMat) this.nexusHullFillMat.color.setHex(p.hex);
        if (this.nexusHullInnerWireMat) this.nexusHullInnerWireMat.color.setHex(p.hex);
        if (this.nexusHeadOutlineMat) this.nexusHeadOutlineMat.color.setHex(p.hex3);
        if (this.nexusHullPointsMat) this.nexusHullPointsMat.color.setHex(p.hex3);
        if (this.nexusRibMat) this.nexusRibMat.color.setHex(p.hex3);
        if (this.nexusGridMat) this.nexusGridMat.color.setHex(p.hex);
        if (this.nexusTentacleLinkMat) this.nexusTentacleLinkMat.color.setHex(p.hex);
        if (this.nexusTentaclePointMat) this.nexusTentaclePointMat.color.setHex(p.hex3);
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
        // Pupil and catchlight are fixed (real "eye" material) and stay untouched here.
        if (this.arxLogosShellRingMats[0]) this.arxLogosShellRingMats[0].color.setHex(p.hex2);
        if (this.arxLogosShellRingMats[1]) this.arxLogosShellRingMats[1].color.setHex(p.hex3);
        if (this.arxLogosSwarmMat) this.arxLogosSwarmMat.color.setHex(p.hex);

        // R.E.D. 9000 (the obsidian core is a fixed material, not theme-tinted -- see buildRed9000Avatar)
        if (this.redLensOuter) this.redLensOuter.material.color.setHex(p.hex);
        if (this.redBlueCircle) this.redBlueCircle.material.color.setHex(p.hex2);
        if (this.redCyanCircle) this.redCyanCircle.material.color.setHex(p.hex3);
        // redLensGlow / redEyeLight are fixed (real "hot lens" glow, like the obsidian core
        // itself) and stay untouched here.

        // A1ter_nul (Cunningham): each shard's glass fill is a fixed obsidian material
        // (see buildAltAvatar) -- only the glowing edges retint, ramped from hex2 at the
        // bottom of the stack to hex3 at the top so the equalizer reads as a spectrum,
        // like a real EQ's color ramp, rather than one flat color.
        const altBottom = new THREE.Color(p.hex2);
        const altTop = new THREE.Color(p.hex3);
        this.altShards.forEach((shard, i) => {
            const tt = this.altShards.length > 1 ? i / (this.altShards.length - 1) : 0;
            shard.outlineMat.color.copy(altBottom).lerp(altTop, tt);
        });
        if (this.altFirewallRingMat) this.altFirewallRingMat.color.setHex(p.hex);
        if (this.altShieldFillMat) this.altShieldFillMat.color.setHex(p.hex);
        if (this.altShieldOutlineMat) this.altShieldOutlineMat.color.setHex(p.hex3);

        // Registered avatars retint too -- all of them, not just the visible one, so
        // switching to one later shows it already in the right colours.
        this.plugins.forEach((entry) => {
            if (typeof entry.def.applyPalette !== 'function') return;
            try {
                entry.def.applyPalette(entry.model, p, THREE);
            } catch (err) {
                console.error(`Avatar "${entry.def.id}" failed to apply the colour palette:`, err);
            }
        });
    }

    /* Build every avatar that registered itself before this engine started. A broken
       one is reported and dropped rather than taking the HUD down with it: someone
       else's avatar file is exactly the code most likely to have a mistake in it, and
       the cost of that must not be a blank window. */
    buildRegisteredAvatars() {
        HologramAvatar.avatarPlugins.forEach((def, id) => {
            try {
                const model = def.build(this.avatarApi());
                if (!model || !model.group) {
                    console.error(`Avatar "${id}": build() must return an object with a .group`);
                    return;
                }
                model.group.visible = false;
                this.scene.add(model.group);
                this.plugins.set(id, { def, model });
            } catch (err) {
                console.error(`Avatar "${id}" failed to build and was skipped:`, err);
            }
        });
    }

    /* What an avatar file is handed. Deliberately small: the three.js module, the
       palette, the helper textures this engine already has, and read-only facts about
       what the companion is doing. An avatar cannot reach the chat, the database or
       the tools from here, and should not need to. */
    avatarApi() {
        return {
            THREE,
            palette: this.activePalette,
            helpers: {
                glowTexture: (size) => this.createGlowSpriteTexture(size),
                radialGlowTexture: (size, center, edge) => this.createRadialGlowTexture(size, center, edge),
                ringTexture: (size, thickness) => this.createRingSpriteTexture(size, thickness),
                hexVertices: (r, rot, cx, cy) => this.hexVertices(r, rot, cx, cy),
            },
        };
    }

    setState(newState) {
        this.state = newState;
    }

    updateAudioData(dataArray) {
        this.audioData = dataArray;
    }

    /* Stop this engine and give back what it holds. Only something that builds an
       engine more than once needs this -- the HUD builds exactly one and keeps it. */
    dispose() {
        this.disposed = true;
        if (this.resizeObserver) this.resizeObserver.disconnect();
        if (this.renderer) {
            this.renderer.dispose();
            if (this.renderer.domElement && this.renderer.domElement.parentElement) {
                this.renderer.domElement.parentElement.removeChild(this.renderer.domElement);
            }
        }
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

/* ---- Bringing your own avatar -------------------------------------------------
 *
 * An avatar is a plain object with an id, a build() that returns something to show,
 * and an animate() called once a frame. Registering has to happen before an engine
 * is constructed -- that is, load your file after core.js and before app.js -- which
 * is the same load-order rule the built-in avatars already follow.
 *
 * See js/hologram/README.md for the full contract and avatar-template.js for a
 * working file to copy.
 */
HologramAvatar.avatarPlugins = new Map();

HologramAvatar.registerAvatar = function (def) {
    if (!def || typeof def !== 'object') {
        console.error('registerAvatar needs an object describing the avatar');
        return false;
    }
    if (typeof def.id !== 'string' || !def.id.trim()) {
        console.error('registerAvatar needs an id, e.g. { id: "my-avatar" }');
        return false;
    }
    if (typeof def.build !== 'function') {
        console.error(`Avatar "${def.id}" has no build() function`);
        return false;
    }
    if (HologramAvatar.avatarPlugins.has(def.id)) {
        /* Replacing rather than refusing: the workbench reloads the same avatar file
           over and over while you work on it. */
        console.warn(`Avatar "${def.id}" was already registered -- replacing it`);
    }
    HologramAvatar.avatarPlugins.set(def.id, def);
    return true;
};

/* What is on offer, for anything drawing an avatar picker. */
HologramAvatar.registeredAvatars = function () {
    return Array.from(HologramAvatar.avatarPlugins.values())
        .map((def) => ({ id: def.id, label: def.label || def.id }));
};

window.HologramAvatar = HologramAvatar;
