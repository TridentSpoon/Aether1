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

    // A single glowing glyph on a transparent canvas (used by The Nexus's letter rain).
    // Rendered white so material.color can tint it per the active color theme.
    createLetterSpriteTexture(char, size = 64) {
        const canvas = document.createElement('canvas');
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext('2d');
        ctx.font = `bold ${Math.round(size * 0.7)}px "Share Tech Mono", monospace`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.shadowColor = 'rgba(255,255,255,0.9)';
        ctx.shadowBlur = size * 0.15;
        ctx.fillStyle = '#ffffff';
        ctx.fillText(char, size / 2, size / 2 + size * 0.03);
        return new THREE.CanvasTexture(canvas);
    }

    // Regular hexagon helpers (used by A.R.X.LOGOS). rotationOffset=0 gives flat top/bottom,
    // pointy left/right; rotationOffset=Math.PI/2 gives pointy top/bottom, flat left/right.
    hexVertices(radius, rotationOffset, cx = 0, cy = 0) {
        const pts = [];
        for (let i = 0; i < 6; i++) {
            const angle = rotationOffset + i * (Math.PI / 3);
            pts.push(new THREE.Vector3(cx + Math.cos(angle) * radius, cy + Math.sin(angle) * radius, 0));
        }
        return pts;
    }

    buildHexOutline(radius, rotationOffset, material, cx = 0, cy = 0) {
        const geom = new THREE.BufferGeometry().setFromPoints(this.hexVertices(radius, rotationOffset, cx, cy));
        return new THREE.LineLoop(geom, material);
    }

    buildHexFill(radius, rotationOffset, material, cx = 0, cy = 0) {
        const geom = new THREE.CircleGeometry(radius, 6, rotationOffset);
        const mesh = new THREE.Mesh(geom, material);
        mesh.position.set(cx, cy, 0);
        return mesh;
    }

    // Builds one flat-ish irregular polygon "shard" (used by A.R.X.LIMES). points2D form a
    // convex loop in local space; the loop is bulged along Z (root at y=0 stays flat, the
    // outward tip recedes) so a cluster of shards reads as facets of one convex dome. Returns
    // a Group containing both the translucent fill and a bright edge outline.
    buildPolygonShard(points2D, bulge, fillMat, outlineMat) {
        // Shapes are drawn with their near (hub-facing) edge at y=0 and extend toward +y.
        // Bulge by y alone (not radial distance) so the whole near edge sits flush at z=0
        // and only the far edge angles backward -- not the near edge's side corners too.
        const maxY = Math.max(...points2D.map(p => p.y), 1);
        const verts = points2D.map(p => {
            return new THREE.Vector3(p.x, p.y, -bulge * (p.y / maxY));
        });

        const positions = [];
        for (let i = 1; i < verts.length - 1; i++) {
            positions.push(verts[0].x, verts[0].y, verts[0].z);
            positions.push(verts[i].x, verts[i].y, verts[i].z);
            positions.push(verts[i + 1].x, verts[i + 1].y, verts[i + 1].z);
        }
        const geom = new THREE.BufferGeometry();
        geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
        geom.computeVertexNormals();
        const fillMesh = new THREE.Mesh(geom, fillMat);

        const outlineGeom = new THREE.BufferGeometry().setFromPoints(verts);
        const outline = new THREE.LineLoop(outlineGeom, outlineMat);

        const group = new THREE.Group();
        group.add(fillMesh);
        group.add(outline);
        return group;
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

        // Shared materials -- one bright outline tone threads through every shard.
        this.arxLimesHubFillMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending
        });
        this.arxLimesOutlineMat = new THREE.LineBasicMaterial({ color: 0xff3300, transparent: true, opacity: 0.95 });
        this.arxLimesMainFillMat = new THREE.MeshBasicMaterial({
            color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
        });
        this.arxLimesWingFillMat = new THREE.MeshBasicMaterial({
            color: 0xff5500, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
        });

        // --- Central Hub: small, semi-transparent faceted anchor. Touches nothing. ---
        // Geodesic (icosahedron, subdivided once) reads as a rounder, more eye-lens-like
        // gem than a plain 8-face octahedron, while still staying faceted rather than smooth.
        const hubGeom = new THREE.IcosahedronGeometry(9, 1);
        this.arxLimesHubMesh = new THREE.Mesh(hubGeom, this.arxLimesHubFillMat);
        const hubEdges = new THREE.EdgesGeometry(hubGeom, 12);
        this.arxLimesHubOutline = new THREE.LineSegments(hubEdges, this.arxLimesOutlineMat);
        this.arxLimesGroup.add(this.arxLimesHubMesh);
        this.arxLimesGroup.add(this.arxLimesHubOutline);

        // --- Plate shapes, all defined locally extending toward +Y ("outward"), so every
        // plate can share the same angle -> world orientation formula regardless of type. ---
        const topShape = [{ x: -16, y: 0 }, { x: 16, y: 0 }, { x: 8, y: 13 }, { x: -8, y: 13 }];
        // Bottom anchor is larger than the top one, per spec. Both are wide and shallow --
        // shaped like eyelid arcs hugging the hub -- rather than tall narrow trapezoids.
        const bottomShape = [{ x: -19, y: 0 }, { x: 19, y: 0 }, { x: 10, y: 16 }, { x: -10, y: 16 } ];
        // Elongated, irregular wing blade swept toward +X; mirrored (negate x) for the left side.
        const wingShapeRight = [{ x: -2, y: 0 }, { x: 2, y: 0 }, { x: 17, y: 26 }, { x: 4, y: 31 }];
        const wingShapeLeft = wingShapeRight.map(p => ({ x: -p.x, y: p.y }));

        const addPlate = (shape, fillMat, worldAngleDeg, tier, radius) => {
            const worldAngle = worldAngleDeg * Math.PI / 180;
            // buildPolygonShard already angles the far (outward) edge backward in Z while
            // keeping the near (hub-facing) edge flush -- no extra rotation needed here.
            const plateGroup = this.buildPolygonShard(shape, 10, fillMat, this.arxLimesOutlineMat);
            plateGroup.position.set(Math.cos(worldAngle) * radius, Math.sin(worldAngle) * radius, 0);
            plateGroup.rotation.z = worldAngle - Math.PI / 2;
            this.arxLimesGroup.add(plateGroup);
            this.arxLimesPlates.push({
                group: plateGroup,
                baseAngle: worldAngle,
                baseRadius: radius,
                phase: worldAngleDeg * 0.03,
                tier
            });
        };

        // Top/bottom hug close to the hub like eyelids; the 4 wings sit further out near the
        // horizontal corners (0/180 deg) like the pointed outer corners/lashes of an eye --
        // together giving the whole cluster a wide, almond-eye silhouette.
        addPlate(topShape, this.arxLimesMainFillMat, 90, 'main', 16);
        addPlate(bottomShape, this.arxLimesMainFillMat, -90, 'main', 16);
        addPlate(wingShapeRight, this.arxLimesWingFillMat, 15, 'wing', 26);
        addPlate(wingShapeRight, this.arxLimesWingFillMat, -15, 'wing', 26);
        addPlate(wingShapeLeft, this.arxLimesWingFillMat, 165, 'wing', 26);
        addPlate(wingShapeLeft, this.arxLimesWingFillMat, -165, 'wing', 26);

        this.scene.add(this.arxLimesGroup);
    }

    buildNexusAvatar() {
        this.nexusGroup = new THREE.Group();

        // --- NEXUS letter rain: real glyphs (not abstract points), falling straight down
        // and wrapping top-to-bottom. This replaces the old inward-spiral vortex entirely. ---
        const letters = ['N', 'E', 'X', 'U', 'S'];
        this.nexusRainMaterials = letters.map(ch => new THREE.SpriteMaterial({
            map: this.createLetterSpriteTexture(ch),
            color: 0x00ff66,
            transparent: true,
            opacity: 0.8,
            blending: THREE.AdditiveBlending,
            depthWrite: false
        }));

        const rainCount = 90;
        const rainHalfWidth = 130;
        const rainTopY = 110;
        const rainBottomY = -110;
        for (let i = 0; i < rainCount; i++) {
            const mat = this.nexusRainMaterials[Math.floor(Math.random() * letters.length)];
            const sprite = new THREE.Sprite(mat);
            const scale = 9 + Math.random() * 7;
            sprite.scale.set(scale, scale, 1);
            sprite.position.set(
                (Math.random() - 0.5) * rainHalfWidth * 2,
                rainTopY + Math.random() * (rainTopY - rainBottomY),
                (Math.random() - 0.5) * 110
            );
            sprite.userData = {
                speed: 26 + Math.random() * 38,
                flickerPhase: Math.random() * Math.PI * 2
            };
            this.nexusRainDrops.push(sprite);
            this.nexusGroup.add(sprite);
        }

        // --- Squid/brain creature: a mantle-like head with trailing tentacles, held in its
        // own sub-group so it can rotate independently to hunt the cursor without spinning
        // the rain field along with it. ---
        this.nexusCreatureGroup = new THREE.Group();

        const headGeom = new THREE.IcosahedronGeometry(17, 1);
        headGeom.scale(1, 0.82, 1.2);
        const headMat = new THREE.MeshBasicMaterial({
            color: 0x00ff66, transparent: true, opacity: 0.22, blending: THREE.AdditiveBlending
        });
        this.nexusHeadMesh = new THREE.Mesh(headGeom, headMat);
        this.nexusCreatureGroup.add(this.nexusHeadMesh);

        const headEdges = new THREE.EdgesGeometry(headGeom, 15);
        const headOutlineMat = new THREE.LineBasicMaterial({ color: 0x00ffaa, transparent: true, opacity: 0.9 });
        this.nexusHeadOutline = new THREE.LineSegments(headEdges, headOutlineMat);
        this.nexusCreatureGroup.add(this.nexusHeadOutline);

        // Tentacles trail behind (local +Z) while the head faces local -Z toward the cursor.
        const tentacleCount = 6;
        const segmentsPerTentacle = 7;
        this.nexusTentacleMat = new THREE.MeshBasicMaterial({
            color: 0x00ff66, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
        });
        for (let t = 0; t < tentacleCount; t++) {
            const baseAngle = (t / tentacleCount) * Math.PI * 2;
            const segments = [];
            for (let s = 0; s < segmentsPerTentacle; s++) {
                const size = 4.5 * (1 - s / segmentsPerTentacle) + 1;
                const geom = new THREE.SphereGeometry(size, 8, 8);
                const seg = new THREE.Mesh(geom, this.nexusTentacleMat);
                this.nexusCreatureGroup.add(seg);
                segments.push(seg);
            }
            this.nexusTentacles.push({ segments, baseAngle, spreadRadius: 9 });
        }

        this.nexusGroup.add(this.nexusCreatureGroup);
        this.scene.add(this.nexusGroup);
    }

    buildRed9000Avatar() {
        this.redGroup = new THREE.Group();

        // 1. Obsidian inner core (HAL / Reactive Daemon Eye) -- fixed dark glossy material,
        // a touch smaller than the old glowing-red sphere, not theme-tinted.
        const coreGeom = new THREE.SphereGeometry(22, 32, 32);
        const coreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0505,
            specular: 0xff6a55,
            shininess: 90,
            transparent: true,
            opacity: 0.97
        });
        this.redCoreSphere = new THREE.Mesh(coreGeom, coreMat);
        this.redGroup.add(this.redCoreSphere);

        // Outer Tactical Wireframe Lens Shell -- a faint accent, kept sparse and dim so it
        // doesn't mask the obsidian core underneath.
        const lensGeom = new THREE.SphereGeometry(34, 10, 10);
        const lensMat = new THREE.MeshBasicMaterial({
            color: 0xff3355,
            wireframe: true,
            transparent: true,
            opacity: 0.12,
            blending: THREE.AdditiveBlending
        });
        this.redLensOuter = new THREE.Mesh(lensGeom, lensMat);
        this.redGroup.add(this.redLensOuter);

        // 2. Eyelid arcs -- flat partial rings cupping the core from above and below,
        // replacing the old two full tilted orbit rings.
        const blueGeom = new THREE.RingGeometry(58, 62, 48, 1, 0.35, 2.44);
        const blueMat = new THREE.MeshBasicMaterial({
            color: 0x0066ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        this.redBlueCircle = new THREE.Mesh(blueGeom, blueMat);
        this.redGroup.add(this.redBlueCircle);

        const cyanGeom = new THREE.RingGeometry(58, 62, 48, 1, Math.PI + 0.35, 2.44);
        const cyanMat = new THREE.MeshBasicMaterial({
            color: 0x00f0ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending
        });
        this.redCyanCircle = new THREE.Mesh(cyanGeom, cyanMat);
        this.redGroup.add(this.redCyanCircle);

        this.scene.add(this.redGroup);
    }

    buildArxLogosAvatar() {
        this.arxLogosGroup = new THREE.Group();

        // --- Centerpiece: large regular hexagon, flat top/bottom, pointy left/right ---
        const centralRadius = 26;
        const centralFillMat = new THREE.MeshBasicMaterial({
            color: 0xe024c3,
            transparent: true,
            opacity: 0.22,
            blending: THREE.AdditiveBlending
        });
        const centralOutlineMat = new THREE.LineBasicMaterial({
            color: 0xe024c3,
            transparent: true,
            opacity: 0.95
        });
        this.arxLogosCentralFill = this.buildHexFill(centralRadius, 0, centralFillMat);
        this.arxLogosCentralOutline = this.buildHexOutline(centralRadius, 0, centralOutlineMat);
        this.arxLogosGroup.add(this.arxLogosCentralFill);
        this.arxLogosGroup.add(this.arxLogosCentralOutline);

        // --- Spiraling arms: 6 identical arms radiating from the central hex's 6 flat edges ---
        this.arxLogosArmMatNear = new THREE.LineBasicMaterial({ color: 0xe024c3, transparent: true, opacity: 0.9 });
        this.arxLogosArmMatMid = new THREE.LineBasicMaterial({ color: 0x9d00ff, transparent: true, opacity: 0.85 });
        this.arxLogosArmMatFar = new THREE.LineBasicMaterial({ color: 0xff00ff, transparent: true, opacity: 0.8 });

        const hexPerArm = 8;
        const apothem = centralRadius * Math.cos(Math.PI / 6); // distance from center to a flat edge

        for (let a = 0; a < 6; a++) {
            const baseAngle = (Math.PI / 6) + a * (Math.PI / 3); // edge-normal directions: 30,90,...,330 deg
            let curAngle = baseAngle;
            let hexSize = 15;
            let curDist = apothem + hexSize * 0.8;

            for (let i = 0; i < hexPerArm; i++) {
                const mat = i < 3 ? this.arxLogosArmMatNear : (i < 6 ? this.arxLogosArmMatMid : this.arxLogosArmMatFar);
                const x = Math.cos(curAngle) * curDist;
                const y = Math.sin(curAngle) * curDist;
                const hex = this.buildHexOutline(hexSize, 0, mat, x, y);
                hex.userData = { armIndex: a, stepIndex: i, baseX: x, baseY: y, phase: a * 0.9 + i * 0.5 };
                this.arxLogosArmHexes.push(hex);
                this.arxLogosGroup.add(hex);

                // Clockwise spiral: tight curve near the core, straightening toward the tail
                const angleStep = (0.62 * Math.pow(0.7, i));
                curAngle -= angleStep;
                curDist += hexSize * 1.3;
                hexSize *= 0.8;
            }
        }

        // --- Outer boundary ring: large hex frame, rotated 90 deg, traced by tiny dot-hexagons ---
        const outerRadius = 112;
        const dotsPerEdge = 10;
        this.arxLogosOuterDotMat = new THREE.MeshBasicMaterial({
            color: 0xff00ff,
            transparent: true,
            opacity: 0.75,
            blending: THREE.AdditiveBlending
        });
        const outerVerts = this.hexVertices(outerRadius, Math.PI / 2);
        for (let e = 0; e < 6; e++) {
            const v0 = outerVerts[e];
            const v1 = outerVerts[(e + 1) % 6];
            for (let d = 0; d < dotsPerEdge; d++) {
                const t = d / dotsPerEdge;
                const x = v0.x + (v1.x - v0.x) * t;
                const y = v0.y + (v1.y - v0.y) * t;
                const dot = this.buildHexFill(2.6, 0, this.arxLogosOuterDotMat, x, y);
                dot.userData = { baseScale: 1.0, phase: (e * dotsPerEdge + d) * 0.35 };
                this.arxLogosOuterDots.push(dot);
                this.arxLogosGroup.add(dot);
            }
        }

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
            // A.R.X.LOGOS: CENTRAL HEXAGON WITH SIX SPIRALING HEXAGON ARMS
            // ==============================================================
            if (this.arxLogosGroup) {
                const spinSpeed = this.state === 'THINKING' ? 0.02 : (this.state === 'SPEAKING' ? 0.01 : 0.004);
                this.arxLogosGroup.rotation.z -= spinSpeed; // clockwise, matching the arm winding
                this.arxLogosGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.1 + this.mouseY;
                this.arxLogosGroup.rotation.y = Math.cos(elapsedTime * 0.35) * 0.1 + this.mouseX;
            }

            if (this.arxLogosCentralFill && this.arxLogosCentralOutline) {
                let coreScale = 1.0;
                if (this.state === 'SPEAKING') {
                    coreScale = 1.0 + audioIntensity * 0.5;
                } else if (this.state === 'THINKING') {
                    coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.18;
                } else {
                    coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
                }
                this.arxLogosCentralFill.scale.set(coreScale, coreScale, coreScale);
                this.arxLogosCentralOutline.scale.set(coreScale, coreScale, coreScale);
            }

            // Arm hexagons — energy pulses outward along each arm while speaking, gentle
            // synchronized breathing otherwise.
            this.arxLogosArmHexes.forEach(hex => {
                let pulseFactor = 1.0;
                if (this.state === 'SPEAKING') {
                    const fVal = (this.audioData[hex.userData.stepIndex % 16] || 0) / 255;
                    pulseFactor = 1.0 + fVal * 0.6 + Math.sin(elapsedTime * 10 + hex.userData.phase) * 0.15;
                } else if (this.state === 'THINKING') {
                    pulseFactor = 1.0 + Math.sin(elapsedTime * 12 + hex.userData.phase) * 0.3;
                } else {
                    pulseFactor = 1.0 + Math.sin(elapsedTime * 2.5 + hex.userData.phase) * 0.08;
                }
                hex.scale.set(pulseFactor, pulseFactor, pulseFactor);
            });

            // Outer dotted boundary ring — subtle shimmer
            this.arxLogosOuterDots.forEach(dot => {
                const shimmer = 1.0 + Math.sin(elapsedTime * 2 + dot.userData.phase) * (this.state === 'SPEAKING' ? 0.35 : 0.15);
                dot.scale.set(shimmer, shimmer, shimmer);
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

            // Eyelid arcs stay static, cupping the core -- only a faint audio-reactive
            // opacity flicker while speaking, no continuous rotation.
            const lidOpacity = this.state === 'SPEAKING' ? 0.85 + audioIntensity * 0.15 : 0.85;
            if (this.redBlueCircle) this.redBlueCircle.material.opacity = lidOpacity;
            if (this.redCyanCircle) this.redCyanCircle.material.opacity = lidOpacity;

        } else if (this.currentAvatar === 'nexus' || this.currentAvatar === 'matrix') {
            // ==============================================================
            // THE NEXUS: SQUID/BRAIN HUNTING THE CURSOR + NEXUS LETTER RAIN
            // ==============================================================

            // Rain falls straight down and wraps top-to-bottom -- no inward spiral/vortex.
            const rainSpeedMult = this.state === 'THINKING' ? 1.8 : (this.state === 'SPEAKING' ? 1.3 : 1.0);
            this.nexusRainDrops.forEach(drop => {
                drop.position.y -= drop.userData.speed * 0.016 * rainSpeedMult;
                if (drop.position.y < -110) {
                    drop.position.y = 110;
                    drop.position.x = (Math.random() - 0.5) * 260;
                }
                drop.material.opacity = 0.55 + Math.sin(elapsedTime * 4 + drop.userData.flickerPhase) * 0.25;
            });

            // Hunt the cursor: ease the creature's facing toward the mouse instead of
            // snapping to it, for a predatory tracking feel. No idle spin.
            if (this.nexusCreatureGroup) {
                const targetYaw = this.mouseX * 2.2;
                const targetPitch = this.mouseY * 1.6;
                this.nexusFacing.yaw += (targetYaw - this.nexusFacing.yaw) * 0.04;
                this.nexusFacing.pitch += (targetPitch - this.nexusFacing.pitch) * 0.04;
                this.nexusCreatureGroup.rotation.y = this.nexusFacing.yaw;
                this.nexusCreatureGroup.rotation.x = this.nexusFacing.pitch;
            }

            if (this.nexusHeadMesh) {
                let headScale = 1.0;
                if (this.state === 'SPEAKING') {
                    headScale = 1.0 + audioIntensity * 0.3;
                } else if (this.state === 'THINKING') {
                    headScale = 1.0 + Math.sin(elapsedTime * 10) * 0.08;
                } else {
                    headScale = 1.0 + Math.sin(elapsedTime * 2) * 0.04;
                }
                this.nexusHeadMesh.scale.set(headScale, headScale, headScale);
                if (this.nexusHeadOutline) this.nexusHeadOutline.scale.set(headScale, headScale, headScale);
            }

            // Tentacles fan outward from the head along their own angle, each undulating
            // perpendicular to its own length, and all trailing backward (+Z, away from
            // whatever the head is currently facing) and slightly down, like flowing behind
            // a creature swimming through the code rain.
            const waveSpeed = this.state === 'SPEAKING' ? 6 : (this.state === 'THINKING' ? 4.5 : 3);
            this.nexusTentacles.forEach(tentacle => {
                const dirX = Math.cos(tentacle.baseAngle);
                const dirY = Math.sin(tentacle.baseAngle) * 0.6;
                const perpX = -dirY;
                const perpY = dirX;
                tentacle.segments.forEach((seg, sIdx) => {
                    const along = sIdx + 1;
                    const wavePhase = elapsedTime * waveSpeed + tentacle.baseAngle * 3;
                    const sway = Math.sin(wavePhase - along * 0.7) * (along * 0.9);
                    const outDist = tentacle.spreadRadius + along * 2.8;
                    seg.position.set(
                        dirX * outDist + perpX * sway,
                        dirY * outDist - along * 1.0 + perpY * sway * 0.5,
                        along * 4.0 + Math.sin(wavePhase * 0.6) * 2
                    );
                });
            });

        } else if (this.currentAvatar === 'arx-limes') {
            // ==========================================
            // A.R.X.LIMES: FLOATING HUB + FRACTURED DOME PLATES
            // ==========================================
            if (this.arxLimesGroup) {
                // No Y-axis spin -- the eye stays facing forward, only tilting to "look around".
                this.arxLimesGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.12 + this.mouseY;
                this.arxLimesGroup.rotation.z = this.mouseX * 0.5;
            }

            if (this.arxLimesHubOutline) {
                let hubScale = 1.0;
                if (this.state === 'SPEAKING') {
                    hubScale = 1.0 + audioIntensity * 0.9;
                } else if (this.state === 'THINKING') {
                    hubScale = 1.0 + Math.sin(elapsedTime * 18) * 0.35;
                } else {
                    hubScale = 1.0 + Math.sin(elapsedTime * 3) * 0.12;
                }
                this.arxLimesHubOutline.scale.set(hubScale, hubScale, hubScale);
                if (this.arxLimesHubMesh) this.arxLimesHubMesh.scale.set(hubScale, hubScale, hubScale);
                this.arxLimesHubOutline.rotation.x += 0.015;
                if (this.arxLimesHubMesh) {
                    this.arxLimesHubMesh.rotation.x = this.arxLimesHubOutline.rotation.x;
                }
            }

            // A periodic stylised blink -- the side wing plates flutter shut and open again
            // every few seconds, like eyelashes blinking. Top/bottom "eyelids" stay still.
            const blinkCycle = 4.5;
            const blinkDuration = 0.28;
            const tInCycle = elapsedTime % blinkCycle;
            let blinkScale = 1.0;
            if (tInCycle < blinkDuration) {
                blinkScale = 1.0 - Math.sin((tInCycle / blinkDuration) * Math.PI) * 0.92;
            }

            // Plates stay put -- static, floating in fixed position -- with only a faint
            // audio-reactive nudge while speaking. No idle/thinking bob.
            this.arxLimesPlates.forEach((plate, idx) => {
                let radiusMult = 1.0;
                if (this.state === 'SPEAKING') {
                    const fVal = (this.audioData[idx % 16] || 0) / 255;
                    radiusMult = 1.0 + fVal * 0.06;
                }
                const r = plate.baseRadius * radiusMult;
                plate.group.position.set(Math.cos(plate.baseAngle) * r, Math.sin(plate.baseAngle) * r, 0);

                if (plate.tier === 'wing') {
                    // Collapse to a thin sliver and back -- closer to how a blinking eyelash
                    // reads than shrinking the whole blade toward the hub.
                    plate.group.scale.set(blinkScale, 1, 1);
                }
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
