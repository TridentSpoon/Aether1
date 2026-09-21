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
 * 7. A1: Monogram Placeholder -- Extruded Solid 'A' beside a Point-Cloud '1', shown before
 *    an operator has actually picked a persona/avatar
 * 8. Real-time Audio Frequency and State deformation.
 *
 * Three of the avatars above belong to named families, grouped together (not scattered
 * through the list above) in index.html's avatar-menu and js/avatar-lab.js's BUILT_IN --
 * see registerAvatar's optional def.group if a new one joins as a plugin rather than a
 * hand-modelled buildXAvatar() like these:
 * - "Singular Ascended Class": hAlcy (1), R.E.D. 9000 (4), A1ter_nul (6).
 * - "Trace Protocols": The Nexus (3), and the plugins Nexus Sent, White Rabbit and Operator.
 *   The latter three are also hidden easter eggs, not just a group: they start absent from
 *   the picker and only ever appear by being triggered from a chat message while The Nexus
 *   is on screen (a greeting, an Alice/rabbit-hole reference, a security or diagnostic
 *   question), unlocking permanently into the picker the first time that happens -- see
 *   js/app.js's EASTER_EGG_RULES, checkEasterEggTriggers and flashEasterEgg.
 * - "The Umbrals": A.R.X.LIMES (2) and A.R.X.LOGOS (5), the A.R.X. name in general.
 * - "The eXcelsior Class": the plugin enXephalon. index.html's avatar-menu keeps a family
 *   heading off screen while its grid is empty, which is how this one waited for its first
 *   avatar.
 *
 * This file defines the class shell (construction, lifecycle, avatar/theme selection).
 * Geometry construction lives in avatar-*.js, shared geometry helpers in geometry-helpers.js,
 * and the per-frame animation loop in animate.js -- all attached via HologramAvatar.prototype,
 * so load order in index.html matters: this file first, then the rest, before app.js runs.
 *
 * The avatar-*.js files are the exception: they are not in the page at launch at all. Each is
 * fetched the first time its avatar is picked -- see avatar-loader.js and materialiseAvatar
 * below -- so an avatar that exists but has never been chosen costs nothing to start up.
 */

class HologramAvatar {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.state = 'IDLE'; // IDLE, LISTENING, THINKING, SPEAKING
        this.currentAvatar = 'a1'; // a1, halcy, arx-limes, nexus, red, arx-logos, alt
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
        this.arxLimesHubRimMat = null; // fixed hot "event horizon" rim — not theme-tinted
        this.arxLimesOutlineMat = null;
        this.arxLimesMainFillMat = null;
        this.arxLimesWingFillMat = null;
        this.arxLimesAccretionGroup = null;
        this.arxLimesAccretionRing1 = null;
        this.arxLimesAccretionRing1Mat = null; // fixed hot accent — not theme-tinted
        this.arxLimesAccretionRing2 = null;
        this.arxLimesAccretionRing2Mat = null; // fixed hot accent — not theme-tinted
        this.arxLimesAccretionHotspots = []; // { mesh, angle, radius, speed }
        this.arxLimesAccretionHotspotsMat = null; // fixed hot accent — not theme-tinted

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
        this.redInnerGlow = null; // same warm gradient, depth-test off, bleeds past the core's silhouette
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
        // behind a static firewall/ICE perimeter of concentric, fading rings. Each shard's
        // glass fill is a fixed obsidian material; its glowing edge is theme-tinted and,
        // while speaking, driven by its own audio frequency bin -- the stack as a vertical
        // equalizer built out of broken relic glass -- see buildAltAvatar.
        this.altGroup = null;
        this.altShardGroup = null;
        this.altShards = []; // { fillMat, outlineMat, baseOpacity, phase, binIndex }
        this.altFirewallRings = []; // { mesh, mat, fade } -- concentric, outer rings fainter
        // Parent for all but the innermost two firewall rings -- added straight to the
        // scene (not nested under altGroup) so those rings never inherit the avatar's
        // own sway/click-nudge and stay genuinely static. See buildAltAvatar.
        this.altFirewallStaticGroup = null;
        this.altShieldGroup = null;
        this.altShieldTiles = []; // { fill, outline, baseAngle, phase }
        this.altShieldFillMat = null;
        this.altShieldOutlineMat = null;

        // Avatars registered from outside this engine -- see registerAvatar below and
        // js/hologram/README.md. Each entry is { def, model }: the definition someone
        // handed us, and whatever their build() returned. A1 (the default placeholder
        // shown before an operator has picked a persona/avatar) is one of these -- see
        // avatar-a1.js -- rather than a hand-modelled built-in like the six below.
        this.plugins = new Map();

        // Which avatars have actually been built into this scene. Avatar files arrive on
        // demand now (see js/hologram/avatar-loader.js), so "built" is no longer the same
        // as "exists" -- this is what stops a second visit to an avatar building it twice.
        this.builtAvatars = new Set();
        // Avatar files currently in flight, so a second ask for one already being fetched
        // waits for it rather than starting another request.
        this.loadingAvatars = new Set();

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

        // Manual zoom -- independent of the auto-fit below, which only ever widens the
        // camera's FOV to keep an avatar from clipping the frame. This is a deliberate
        // override on top of that: how big the avatar reads on screen, picked by hand
        // (see setZoom). Applied as a uniform scale on avatarZoomGroup (see init()), which
        // holds every avatar except the custom one's own tier-4 effect/background layer --
        // that layer is excluded on purpose, see avatar-custom.js.
        this.zoomScale = 1;
        this.avatarZoomGroup = null;
        // The ceiling on setZoom. 2.5 is as far as the HUD's zoom slider goes; the fullscreen
        // face raises it (see setFillFraction), because a monitor is not a panel.
        this.maxZoom = 2.5;

        // Null unless a caller opts in (see setFillFraction): how much of the frame the
        // avatar should be scaled up to occupy. Only the fullscreen face asks for this.
        this.fillFraction = null;

        // Pointer tracking -- only The Nexus consumes these (mouse-manipulated head that
        // autonomously "hunts" when the pointer isn't actively directing it); other avatars
        // stay front-facing/static and simply don't read them.
        this.nexusMouseNX = 0; // -1..1 across the renderer element
        this.nexusMouseNY = 0;
        this.lastMouseMoveTime = -999; // seconds on this.clock

        // Auto-fit: keeps the active avatar's edges (and its idle bounce/pulse/float) clear
        // of the viewport frame, at any container size or aspect ratio, by widening the
        // camera's field of view rather than assuming a fixed distance ever suited every
        // shape and every panel size. this.baseFov is the floor -- the tuned, "normal"
        // zoom every avatar was designed to be seen at -- widened only as far as the
        // container's current shape demands. contentHalfWidth/Height are the active
        // avatar's own measured half-extents (see updateContentFit), read afresh on every
        // avatar switch and reapplied on every resize.
        this.baseFov = 45;
        this.maxFov = 100; // guards against fisheye distortion in a pathologically thin panel
        this.fitMargin = 1.3; // 30% breathing room beyond the avatar's resting bounding box
        this.fitBounceAllowance = 35; // world units -- covers bounce/pulse/click motion a resting bounding box doesn't capture
        this.contentHalfWidth = null;
        this.contentHalfHeight = null;
        // The same measurement with the avatar's glow left out -- only the fill path below
        // uses these (see expandBySolidParts and applyFillZoom).
        this.solidHalfWidth = null;
        this.solidHalfHeight = null;

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
        this.camera = new THREE.PerspectiveCamera(this.baseFov, width / height, 0.1, 1000);
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

        // Every avatar except the custom one's tier-4 effect layer is built straight into
        // this group (see the avatar-*.js buildXAvatar() methods and buildRegisteredAvatars
        // below) instead of directly into the scene, purely so setZoom has one uniform scale
        // to apply no matter which avatar is active -- it composes for free with each
        // avatar's own rotation/position/local scale since it's just another ancestor in
        // the transform chain.
        this.avatarZoomGroup = new THREE.Group();
        this.scene.add(this.avatarZoomGroup);

        /* Every avatar used to be built here, all nineteen of them, to show one. Now only
           whatever is already in the page gets built -- which at startup is the avatar being
           worn and nothing else -- and setAvatar below fetches and builds the rest the first
           time somebody picks one. */
        this.buildRegisteredAvatars();

        /* Initial avatar shape + colour setup (independent of each other). Tinting goes
           through applyColorPalette rather than setColorTheme so that a palette handed in
           before build() -- three colours picked by hand, which have no preset id -- is not
           thrown away and replaced by whatever id was last named. */
        this.setAvatar(this.currentAvatar);
        this.applyColorPalette();

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
        /* Its file may not be in the page yet -- they arrive on demand (see
           js/hologram/avatar-loader.js). Build it if it is here, otherwise fetch it and come
           back. The rest of this method still runs in the meantime, so the avatar being left
           behind is hidden immediately rather than staying on screen until the new one's file
           finishes loading, which would read as the click not registering. */
        this.materialiseAvatar(avatar);
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
        if (this.altFirewallStaticGroup) this.altFirewallStaticGroup.visible = isAlt;

        // CRT scanline/flicker overlay (see #hologram-viewport.crt-active::after in
        // A1theme.css / sprite.css) -- only The Nexus Crew's wireframe "live sensor feed"
        // look asks for it.
        if (this.container) this.container.classList.toggle('crt-active', isNexus);

        // Chromatic-glitch scanline overlay (see #hologram-viewport.glitch-active::after in
        // A1theme.css / sprite.css) -- only A1ter_nul's digital-ghost look asks for it.
        if (this.container) this.container.classList.toggle('glitch-active', isAlt);

        // A different avatar can be a very different size (a hand-modelled built-in vs. a
        // custom recipe dialled up to its largest settings) -- measure the one now on
        // screen and refit the camera to it.
        const activeObjects = plugin && plugin.model && plugin.model.group
            ? [plugin.model.group]
            : [
                isHalcy && this.particleSystem, isHalcy && this.halcyOuterRing,
                isHalcy && this.halcyInnerRingGroup, isHalcy && this.coreOrb,
                isArxLimes && this.arxLimesGroup, isNexus && this.nexusGroup,
                isRed && this.redGroup, isArxLogos && this.arxLogosGroup, isAlt && this.altGroup
            ].filter(Boolean);
        this.updateContentFit(activeObjects);
    }

    // Measures the given objects' combined world-space bounding box and refits the camera
    // to it (see updateContentFit below) -- called whenever the active avatar changes shape:
    // on avatar selection (setAvatar above) and after a registered avatar rebuilds itself
    // from a changed recipe (rebuildRegisteredAvatar below), since that can resize it while
    // it's already the one on screen.
    updateContentFit(objects) {
        if (!objects || !objects.length) return;

        // Measured at zoomScale's identity, not whatever it currently is -- auto-fit exists
        // to keep an avatar's own resting geometry inside the frame; a manual zoom is a
        // deliberate override on top of that, not something auto-fit should immediately
        // widen the FOV to undo. avatarZoomGroup is the only thing setZoom ever touches, so
        // resetting it here and restoring it after is exact, not an approximation.
        const zoomed = this.avatarZoomGroup && this.avatarZoomGroup.scale.x !== 1;
        if (zoomed) {
            this.avatarZoomGroup.scale.setScalar(1);
            this.avatarZoomGroup.updateMatrixWorld(true);
        }
        const box = new THREE.Box3();
        objects.forEach(obj => box.expandByObject(obj));
        const solid = new THREE.Box3();
        objects.forEach(obj => this.expandBySolidParts(solid, obj));
        if (zoomed) {
            this.avatarZoomGroup.scale.setScalar(this.zoomScale);
            this.avatarZoomGroup.updateMatrixWorld(true);
        }
        if (box.isEmpty()) return;

        // Half-extent in each axis, not assuming the group is centred on the origin -- a1's
        // solid 'A' and point-cloud '1' sit either side of it, for instance.
        this.contentHalfWidth = Math.max(Math.abs(box.min.x), Math.abs(box.max.x));
        this.contentHalfHeight = Math.max(Math.abs(box.min.y), Math.abs(box.max.y));
        this.solidHalfWidth = solid.isEmpty() ? this.contentHalfWidth
            : Math.max(Math.abs(solid.min.x), Math.abs(solid.max.x));
        this.solidHalfHeight = solid.isEmpty() ? this.contentHalfHeight
            : Math.max(Math.abs(solid.min.y), Math.abs(solid.max.y));
        this.applyContentFit();
    }

    /* The same box, minus the haze. Several avatars wear a soft glow -- a big, near-
       transparent sprite or halo several times the size of the thing it is glowing around
       (a1's is a 160-unit sprite at 0.18 opacity around lettering only 80 units tall). For
       auto-fit that is exactly right: the glow is part of the picture and clipping it against
       a panel edge would show. For "fill the screen" it is exactly wrong -- sizing the avatar
       so its glow reaches the edges of a monitor leaves the avatar itself a third of the
       height it should be, which is the postage stamp this whole path exists to avoid.

       So the fill path measures only what is solid enough to read as an edge, and lets the
       haze run off the screen, which is what haze is supposed to do. The threshold is on
       opacity alone: an object you can see through is not where the avatar ends. */
    expandBySolidParts(box, object) {
        object.traverseVisible((node) => {
            if (!node.geometry) return;
            const material = node.material;
            const materials = Array.isArray(material) ? material : [material];
            const solid = materials.some(m => m && (!m.transparent || (m.opacity == null ? 1 : m.opacity) >= 0.5));
            if (solid) box.expandByObject(node);
        });
    }

    // Manual zoom -- how big the avatar reads on screen, picked by hand rather than derived
    // from its geometry. 0.5-2.5x; clamped so the slider driving this can't scale an avatar
    // down to nothing or blow it up past the point of reading as anything. Custom avatar's
    // tier-4 effect/background layer is deliberately outside avatarZoomGroup (see
    // zoomParentFor and avatar-custom.js) so this only ever resizes the avatar itself, tiers
    // 1-3, never its background.
    setZoom(scale) {
        this.zoomScale = Math.min(this.maxZoom, Math.max(0.5, Number(scale) || 1));
        if (this.avatarZoomGroup) this.avatarZoomGroup.scale.setScalar(this.zoomScale);
    }

    // Widens the camera's (vertical) field of view just far enough that the active avatar's
    // measured half-extents -- padded for bounce/pulse motion a resting bounding box can't
    // see -- stay clear of both the left/right edges and the top/bottom edges, at whatever
    // aspect ratio the container currently has. Never narrower than baseFov, so a normally
    // proportioned panel keeps every avatar's tuned "resting" zoom exactly as designed.
    applyContentFit() {
        if (!this.camera || !this.container) return;
        if (this.contentHalfWidth == null || this.contentHalfHeight == null) return;
        const width = this.container.clientWidth;
        const height = this.container.clientHeight;
        if (!width || !height) return;

        const aspect = width / height;
        const distance = this.camera.position.z;
        const halfW = this.contentHalfWidth * this.fitMargin + this.fitBounceAllowance;
        const halfH = this.contentHalfHeight * this.fitMargin + this.fitBounceAllowance;

        // Visible half-height at this distance is distance*tan(fov/2); visible half-width is
        // that times aspect. Solving each for the fov that makes it exactly halfH/halfW gives
        // the field of view each axis would need on its own -- taking the wider of the two
        // satisfies both at once, whichever axis is currently the tighter fit.
        const neededForHeight = 2 * Math.atan(halfH / distance);
        const neededForWidth = 2 * Math.atan(halfW / (distance * aspect));
        const neededFovDeg = THREE.MathUtils.radToDeg(Math.max(neededForHeight, neededForWidth));

        this.camera.fov = Math.min(this.maxFov, Math.max(this.baseFov, neededFovDeg));
        this.camera.updateProjectionMatrix();

        // Auto-fit has just decided how much of the frame the avatar is allowed; if a caller
        // asked to fill it, that is the moment to work out by how much.
        if (this.fillFraction != null) this.applyFillZoom();
    }

    /* "Use the space you have been given."

       Auto-fit above only ever *widens* the field of view, never narrows it: a panel too
       narrow for an avatar gets a wider lens, and a panel with room to spare simply has room
       to spare. That is right for the HUD, where every avatar is meant to read at the resting
       size it was designed at, and wrong for a window whose entire job is to be a face --
       frontend/face.html fills a monitor, and an avatar drawn at panel size in the middle of
       it is a postage stamp on a wall.

       So this is the other half of the pair, and it is opt-in: the face asks for it, nothing
       else does. `fraction` is how much of the frame the avatar's solid parts should take up,
       1 meaning right up to the 30% margin fitMargin reserves for idle motion. Null (the
       default) leaves manual zoom alone entirely, which is what every existing caller wants
       -- the HUD's own zoom slider would fight anything else. */
    setFillFraction(fraction) {
        this.fillFraction = fraction == null ? null : Math.min(1, Math.max(0.1, Number(fraction) || 1));
        // 2.5 is the slider's ceiling, and it is the wrong one here: an avatar whose solid
        // parts are small relative to its glow needs more than that before it reads as a
        // face across a room. 6 is the point past which even the smallest avatar's lettering
        // is filling a 4K screen, so nothing useful lies beyond it.
        this.maxZoom = this.fillFraction == null ? 2.5 : 6;
        // Either recompute the fill, or -- when fill is being switched back off -- put the
        // current zoom back through setZoom so the lowered ceiling actually applies to it.
        if (this.fillFraction != null) this.applyFillZoom();
        else this.setZoom(this.zoomScale);
    }

    applyFillZoom() {
        if (!this.camera || !this.container || this.fillFraction == null) return;
        if (this.contentHalfWidth == null || this.contentHalfHeight == null) return;
        const width = this.container.clientWidth;
        const height = this.container.clientHeight;
        if (!width || !height) return;

        // The avatar's solid parts rather than the box auto-fit uses, and padded by the
        // ordinary margin only. The bounce allowance is deliberately left out: it is a flat
        // 35 world units, sized against an avatar seen in a panel, and an avatar's actual
        // idle motion scales with the zoom applied here, so adding it again on top would
        // reserve a third of a monitor for a wobble of a few pixels. fitMargin's 30% covers
        // the motion at any size, because it is a proportion.
        const halfW = (this.solidHalfWidth == null ? this.contentHalfWidth : this.solidHalfWidth) * this.fitMargin;
        const halfH = (this.solidHalfHeight == null ? this.contentHalfHeight : this.solidHalfHeight) * this.fitMargin;
        if (!halfW || !halfH) return;
        const visibleHalfH = this.camera.position.z * Math.tan(THREE.MathUtils.degToRad(this.camera.fov) / 2);
        const visibleHalfW = visibleHalfH * (width / height);

        // Whichever axis runs out first decides, and never below 1: shrinking an avatar
        // below its designed size is auto-fit's job (by widening the lens), not this one's.
        const fill = Math.min(visibleHalfH / halfH, visibleHalfW / halfW) * this.fillFraction;
        this.setZoom(Math.max(1, fill));
    }

    /* Which color palette tints the currently active (and future) avatar shapes.
       Two ways in, because there are two kinds of caller. A preset id is what the workbench
       and any avatar file written against the documented API pass; an explicit palette is what
       the HUD passes, because its colours are three values someone can pick by hand and there
       is no id for those. */
    setColorTheme(theme) {
        this.currentColorTheme = theme;
        this.setColorPalette(THEME_PALETTES[theme] || THEME_PALETTES.halcy);
    }

    setColorPalette(palette) {
        this.activePalette = palette || THEME_PALETTES.halcy;
        this.applyColorPalette();
    }

    applyColorPalette() {
        const p = this.activePalette;

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

        // A.R.X.LIMES fractured dome. The hub is a fixed black void now (a black hole),
        // and its event-horizon rim + accretion disc are this avatar's fixed hot accent --
        // none of the three retint with the theme, same as every other avatar's one
        // constant identity colour.
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
        this.altFirewallRings.forEach(ring => ring.mat.color.setHex(p.hex));
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
        HologramAvatar.avatarPlugins.forEach((def, id) => this.buildRegisteredAvatar(id));
    }

    /* One registered avatar, built into the scene hidden. Separate from the loop above
       because an avatar file can now arrive long after the engine started -- the moment
       somebody picks it -- and that one has to be built on its own. */
    buildRegisteredAvatar(id) {
        const def = HologramAvatar.avatarPlugins.get(id);
        if (!def || this.plugins.has(id)) return false;
        try {
            const model = def.build(this.avatarApi());
            if (!model || !model.group) {
                console.error(`Avatar "${id}": build() must return an object with a .group`);
                return false;
            }
            model.group.visible = false;
            this.zoomParentFor(id).add(model.group);
            this.plugins.set(id, { def, model });
            this.builtAvatars.add(id);
            return true;
        } catch (err) {
            console.error(`Avatar "${id}" failed to build and was skipped:`, err);
            return false;
        }
    }

    /* Turn a name into geometry in this scene, if its file is here.
     *
     * Returns true once the avatar exists (or already did). False means the file has not
     * arrived, in which case it is fetched and this runs again -- and, if the operator has
     * not picked something else in the meantime, the avatar is shown. The palette is
     * re-applied because a freshly built avatar is wearing whatever colours its own file
     * chose, not the ones the HUD is set to.
     */
    materialiseAvatar(avatar) {
        let id = HologramAvatar.canonicalAvatarId(avatar);
        // A name this engine does not recognise has always shown hAlcy (see setAvatar and
        // animate) -- so that is the file to fetch for it, not one that does not exist.
        if (!HologramAvatar.avatarPlugins.has(id) && HologramAvatar.knownAvatarIds().indexOf(id) === -1) {
            id = 'halcy';
        }
        if (this.builtAvatars.has(id)) return true;

        if (HologramAvatar.avatarPlugins.has(id)) return this.buildRegisteredAvatar(id);

        const builder = HologramAvatar.builderNameFor(id);
        if (builder && typeof this[builder] === 'function') {
            this[builder]();
            this.builtAvatars.add(id);
            return true;
        }

        if (this.loadingAvatars.has(id)) return false; // already on its way
        this.loadingAvatars.add(id);
        HologramAvatar.loadAvatar(id).then(() => {
            this.loadingAvatars.delete(id);
            // An engine thrown away while its avatar was in flight (the workbench disposes
            // one per slider nudge) must not have geometry pushed into it afterwards.
            if (this.disposed) return;
            if (!this.materialiseAvatar(this.currentAvatar)) return; // the file failed; it reported itself
            this.applyColorPalette();
            this.setAvatar(this.currentAvatar);
        });
        return false;
    }

    // Where a registered avatar's top-level group belongs: inside avatarZoomGroup for the
    // uniform manual-zoom treatment every other avatar gets, or straight into the scene for
    // the custom avatar, which scales only its own tier-1-3 parts (see avatar-custom.js) and
    // must keep its tier-4 effect/background layer out of setZoom's reach.
    zoomParentFor(id) {
        return id === 'custom' ? this.scene : this.avatarZoomGroup;
    }

    /* Throw away a registered avatar's model and build it again from its definition.
       build() runs once per engine, which is the right contract for an avatar whose shape
       is fixed in code -- but the custom avatar's shape comes from a recipe that can change
       while the HUD is open. Without this, a design saved in the workbench would not appear
       until the whole app was restarted, which reads exactly like the save not working. */
    rebuildRegisteredAvatar(id) {
        const entry = this.plugins.get(id);
        if (!entry) return false;

        const wasVisible = entry.model.group ? entry.model.group.visible : false;
        if (entry.model.group) {
            if (entry.model.group.parent) entry.model.group.parent.remove(entry.model.group);
            disposeObject3D(entry.model.group);
        }

        try {
            const model = entry.def.build(this.avatarApi());
            if (!model || !model.group) {
                console.error(`Avatar "${id}": build() must return an object with a .group`);
                return false;
            }
            model.group.visible = wasVisible;
            this.zoomParentFor(id).add(model.group);
            /* broken is cleared: the rebuild may be the very fix for whatever threw. */
            this.plugins.set(id, { def: entry.def, model });
        } catch (err) {
            console.error(`Avatar "${id}" failed to rebuild:`, err);
            this.plugins.delete(id);
            return false;
        }

        this.applyColorPalette();
        // The rebuilt shape can be a different size than the one it replaced (the recipe
        // may have changed its radius/size settings) -- if it's the one currently on
        // screen, the camera's fit needs to be measured against the new geometry, not the
        // old one it was computed from.
        if (wasVisible) this.updateContentFit([this.plugins.get(id).model.group]);
        return true;
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
        this.applyContentFit(); // reapplies fov for the new aspect
        this.camera.updateProjectionMatrix(); // belt-and-suspenders: still needed if applyContentFit bailed out (no content measured yet)
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

/* What is on offer, for anything drawing an avatar picker. group is optional and purely
   organisational -- a plugin avatar that belongs to a named family (see "The Umbrals" in
   index.html's avatar-menu and js/avatar-lab.js's BUILT_IN) sets def.group to the same
   family name so pickers can cluster it with its relatives instead of listing it flat. */
HologramAvatar.registeredAvatars = function () {
    return Array.from(HologramAvatar.avatarPlugins.values())
        .map((def) => ({ id: def.id, label: def.label || def.id, group: def.group }));
};

/* Geometry and materials live on the GPU and are not reclaimed by dropping the last
   JavaScript reference to them -- they have to be handed back explicitly. Anything that
   rebuilds repeatedly (the workbench, on every slider nudge) leaks the whole scene
   otherwise. */
function disposeObject3D(root) {
    root.traverse((node) => {
        if (node.geometry) node.geometry.dispose();
        const materials = Array.isArray(node.material) ? node.material : [node.material];
        materials.forEach((material) => {
            if (!material) return;
            if (material.map) material.map.dispose();
            material.dispose();
        });
    });
}

window.HologramAvatar = HologramAvatar;
