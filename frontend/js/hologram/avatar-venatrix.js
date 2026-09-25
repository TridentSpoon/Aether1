/* Avatar: VENATRIX -- The eXcelsior Class's fifth, and the line's hunter. A mirror-chrome
 * helm over a wide mantle, a dark recessed band across the eyes with two hot lenses burning
 * in it, a ring of armour plates that splay open while it works, and a reticle standing off
 * the face that turns and tightens on whatever it is looking at.
 *
 * The idea it is built around is adaptation. Everything else in this line has one shape and
 * animates inside it; this one changes shape. The plates lie flat and the reticle is loose
 * when it is idle, the plates lift and counter-turn while it is thinking, and the reticle
 * closes right down when it is listening to you. What it is doing is legible from across
 * the room without reading a word of the HUD, which is the point of an avatar.
 *
 * Two house conventions it follows. The chrome is fixed and never takes the theme, the same
 * rule C.I.C.E.R.O.'s enamel follows -- a mirrored surface that turned crimson with the HUD
 * would stop reading as metal. And the lenses are a fixed hot colour, like R.E.D. 9000's eye
 * and enXephalon's signature points, because a hunter's optics are its own.
 *
 * The one thing worth knowing before editing the visor: it is a dark inset, not a glowing
 * pane. C.I.C.E.R.O. established why -- additive light over a near-white shell renders white,
 * so the only way a bright feature reads on polished metal is to sink it into something dark
 * first and let the small hot shapes inside it do the glowing.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'venatrix',
    label: 'VENATRIX',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted -- see the header. Chrome, the dark it is inset with,
        // and the hot colour of the optics.
        const CHROME = 0xdfe6f2;
        const SHADOW = 0x090c14;
        const OPTIC = 0xff2d7a;

        const HEAD_HEIGHT = 62;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(180, 180, 1);
        glow.position.z = -40;
        group.add(glow);

        const chromeMat = new THREE.MeshPhongMaterial({
            color: CHROME, shininess: 200, specular: 0xffffff,
        });
        const shadowMat = new THREE.MeshPhongMaterial({
            color: SHADOW, shininess: 40, specular: 0x33405c,
        });

        /* The helm, turned rather than assembled: a profile swept around the vertical axis.
           A lathe is what gets a face-like taper -- narrow crown, cheekbone, jaw drawn back
           in -- out of a dozen numbers instead of a pile of welded primitives. The profile
           runs bottom to top in units of the head's own height, so changing HEAD_HEIGHT
           rescales the whole helm and nothing else has to move. */
        const PROFILE = [
            [0.00, -0.54], [0.13, -0.53], [0.25, -0.47], [0.33, -0.35],
            [0.38, -0.20], [0.40, -0.03], [0.39, 0.13], [0.35, 0.28],
            [0.27, 0.41], [0.15, 0.50], [0.00, 0.56],
        ];
        const headGroup = new THREE.Group();
        headGroup.position.y = 8;
        group.add(headGroup);

        const helm = new THREE.Mesh(
            new THREE.LatheGeometry(
                PROFILE.map(([r, y]) => new THREE.Vector3(r * HEAD_HEIGHT, y * HEAD_HEIGHT, 0)),
                40
            ),
            chromeMat
        );
        headGroup.add(helm);

        /* The visor: a dark band sunk into the helm at eye height, drawn as an open cylinder
           slightly proud of the surface so it does not fight the helm for the same pixels. */
        const visor = new THREE.Mesh(
            new THREE.CylinderGeometry(
                HEAD_HEIGHT * 0.395, HEAD_HEIGHT * 0.405, HEAD_HEIGHT * 0.19, 40, 1, true, -1.15, 2.3
            ),
            shadowMat
        );
        visor.position.y = HEAD_HEIGHT * 0.06;
        headGroup.add(visor);

        // --- The optics. Two hot lenses in the dark band, each with its own bloom. ---
        const opticMat = new THREE.MeshBasicMaterial({
            color: OPTIC, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const opticGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(48), color: OPTIC,
            transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const optics = [-1, 1].map((side) => {
            const eye = new THREE.Group();
            eye.position.set(side * HEAD_HEIGHT * 0.15, HEAD_HEIGHT * 0.06, HEAD_HEIGHT * 0.38);
            // A lozenge rather than a disc: a lens with a long axis has a direction, and a
            // face made of two round dots reads as friendly, which this is not.
            const lens = new THREE.Mesh(new THREE.CircleGeometry(HEAD_HEIGHT * 0.075, 24), opticMat);
            lens.scale.set(1, 0.44, 1);
            eye.add(lens);
            const bloom = new THREE.Sprite(opticGlowMat);
            bloom.scale.set(HEAD_HEIGHT * 0.36, HEAD_HEIGHT * 0.36, 1);
            bloom.position.z = 1.5;
            eye.add(bloom);
            headGroup.add(eye);
            return { eye, lens };
        });

        /* The adaptive plates. Six chrome shells on a ring around the helm, each parented to
           its own pivot at the centre so opening one is a single rotation about an axis that
           already points the right way -- the alternative is solving for a position and an
           angle per plate on every frame for the same picture. */
        const PLATES = 6;
        // Sized to sit on the helm as a segment of a band, not as a tile beside it: a
        // little wider than the gap to its neighbour and thin enough to follow the curve.
        const plateGeom = new THREE.BoxGeometry(HEAD_HEIGHT * 0.22, HEAD_HEIGHT * 0.09, HEAD_HEIGHT * 0.045);
        const plates = [];
        for (let i = 0; i < PLATES; i++) {
            const spin = new THREE.Group();          // where the plate sits on the ring
            spin.rotation.z = (i / PLATES) * Math.PI * 2;
            const hinge = new THREE.Group();         // how far it has lifted off the helm
            hinge.position.y = HEAD_HEIGHT * 0.38;
            const plate = new THREE.Mesh(plateGeom, chromeMat);
            plate.position.y = HEAD_HEIGHT * 0.02;
            hinge.add(plate);
            spin.add(hinge);
            headGroup.add(spin);
            plates.push({ spin, hinge, phase: (i / PLATES) * Math.PI * 2 });
        }

        /* The reticle: two rings and four ticks standing off the face, in the theme rather
           than the optics' fixed hot colour. It is the part of the avatar that answers the
           room, so it is the part that wears the room's colour. */
        const reticle = new THREE.Group();
        // Far enough forward, and wider than the helm, that it frames the face instead of
        // hiding behind it -- a sight you are looking through, not a badge on the forehead.
        reticle.position.z = HEAD_HEIGHT * 0.85;
        headGroup.add(reticle);

        // palette.hex, not hex3: hex3 is the palette's deep channel and a thin additive
        // ring drawn in it is invisible against the HUD's own dark background.
        const reticleMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
        });
        reticle.add(new THREE.Mesh(new THREE.RingGeometry(HEAD_HEIGHT * 0.5, HEAD_HEIGHT * 0.525, 48), reticleMat));
        const inner = new THREE.Mesh(
            new THREE.RingGeometry(HEAD_HEIGHT * 0.3, HEAD_HEIGHT * 0.315, 40), reticleMat
        );
        reticle.add(inner);
        for (let i = 0; i < 4; i++) {
            const tick = new THREE.Mesh(
                new THREE.RingGeometry(HEAD_HEIGHT * 0.37, HEAD_HEIGHT * 0.5, 8, 1, i * Math.PI / 2 - 0.07, 0.14),
                reticleMat
            );
            reticle.add(tick);
        }

        // A crest down the midline of the crown. Without it the turned helm is a smooth
        // ovoid from every angle, and smooth ovoids read as friendly.
        const crest = new THREE.Mesh(
            new THREE.BoxGeometry(HEAD_HEIGHT * 0.045, HEAD_HEIGHT * 0.1, HEAD_HEIGHT * 0.55),
            chromeMat
        );
        crest.position.y = HEAD_HEIGHT * 0.46;
        crest.rotation.x = 0.12;
        headGroup.add(crest);

        // --- The mantle: a turned collar under the helm. It is not a body, and is not meant
        // to be one -- it is the shoulders a head needs to stop looking like it is floating.
        const MANTLE = [
            [0.10, 0.00], [0.34, -0.06], [0.52, -0.16], [0.62, -0.30], [0.66, -0.46],
        ];
        const mantle = new THREE.Mesh(
            new THREE.LatheGeometry(
                MANTLE.map(([r, y]) => new THREE.Vector3(r * HEAD_HEIGHT, y * HEAD_HEIGHT, 0)),
                40
            ),
            chromeMat
        );
        mantle.position.y = -HEAD_HEIGHT * 0.42;
        group.add(mantle);

        // A lit seam where the mantle meets the helm, so the join is a working part rather
        // than a gap, and one more place the theme reaches the chrome.
        const seamMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
        });
        const seam = new THREE.Mesh(
            new THREE.TorusGeometry(HEAD_HEIGHT * 0.36, HEAD_HEIGHT * 0.018, 8, 64), seamMat
        );
        seam.rotation.x = Math.PI / 2;
        seam.position.y = -HEAD_HEIGHT * 0.44;
        group.add(seam);

        /* The scan: one hot line that runs down the helm and starts again at the top, which
           is the whole reason this avatar looks like it is examining something rather than
           waiting. Drawn as a flat ring wider than the helm so it reads at every angle the
           HUD can be dragged to. */
        const scanMat = new THREE.MeshBasicMaterial({
            color: OPTIC, transparent: true, opacity: 0.4,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const scan = new THREE.Mesh(
            new THREE.RingGeometry(HEAD_HEIGHT * 0.4, HEAD_HEIGHT * 0.46, 48), scanMat
        );
        scan.rotation.x = Math.PI / 2;
        headGroup.add(scan);

        return {
            group, headGroup, helm, chromeMat, visor, optics, opticMat, opticGlowMat,
            plates, reticle, reticleMat, inner, mantle, seam, seamMat, scan, scanMat,
            glow, glowMat, headHeight: HEAD_HEIGHT,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';
        const H = model.headHeight;

        model.group.position.y = Math.sin(ctx.time * 0.8) * 2.5;

        // Looking around: a slow sweep that stops and holds on you the moment you speak.
        model.headGroup.rotation.y = isListening
            ? Math.sin(ctx.time * 0.4) * 0.1
            : Math.sin(ctx.time * 0.9) * 0.3;
        model.headGroup.rotation.x = isListening ? 0.1 : Math.cos(ctx.time * 0.6) * 0.06;

        /* The plates. Shut against the helm at rest, lifted and turned while it thinks,
           thrown wide on a click. Each one is a beat behind its neighbour, so the ring
           opens as a wave rather than as one snapping shutter. */
        const opening = (isThinking ? 1 : isListening ? 0.12 : 0.3) + ctx.click;
        model.plates.forEach(({ spin, hinge, phase }, i) => {
            const wave = Math.sin(ctx.time * (isThinking ? 3.4 : 1.2) + phase) * 0.5 + 0.5;
            const lift = Math.min(1.35, opening * (0.35 + wave * 0.65));
            hinge.rotation.x = -lift * 0.7;
            hinge.position.y = H * (0.38 + lift * 0.11);
            spin.rotation.z = (i / model.plates.length) * Math.PI * 2 + lift * 0.12;
        });

        // The optics ride the voice while it speaks and pulse while it thinks; listening
        // narrows them, which is what attention looks like on a face with no brow.
        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 5.5)) * 0.6 : 0;
        model.opticMat.opacity = 0.7 + heat * 0.3;
        model.opticGlowMat.opacity = 0.45 + heat * 0.5 + ctx.click * 0.2;
        model.optics.forEach(({ lens }) => {
            lens.scale.set(1 + heat * 0.18, (isListening ? 0.3 : 0.44) + heat * 0.22, 1);
        });

        /* The reticle turns all the time and tightens when it is listening -- a sight
           closing on a target. The ticks ride the outer ring, so scaling the group is the
           whole gesture. */
        model.reticle.rotation.z = ctx.time * (isThinking ? 1.6 : 0.45);
        const tight = isListening ? 0.72 : 1 - ctx.click * 0.2;
        model.reticle.scale.setScalar(tight + heat * 0.06);
        model.reticleMat.opacity = 0.55 + heat * 0.35 + (isListening ? 0.2 : 0);

        // The scan runs from the crown to the jaw and begins again, faster when there is
        // something to work out. Accumulated, so a change of pace never jumps the line.
        const pace = isThinking ? 1.5 : isListening ? 0.5 : 0.85;
        model.scanPos = ((model.scanPos || 0) + pace * 0.016) % 1;
        model.scan.position.y = H * (0.5 - model.scanPos);
        // Widest across the middle of the helm and gone at both ends, so the line looks
        // like it is tracking the surface rather than passing through it.
        const across = Math.sin(model.scanPos * Math.PI);
        model.scan.scale.setScalar(0.55 + across * 0.55);
        model.scanMat.opacity = 0.12 + across * 0.35;

        model.seamMat.opacity = 0.6 + heat * 0.35;
        model.glowMat.opacity = 0.14 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.04 + 0.04);
    },

    applyPalette(model, palette) {
        model.reticleMat.color.setHex(palette.hex);
        model.seamMat.color.setHex(palette.hex2);
        model.glowMat.color.setHex(palette.hex);
        // The chrome, the dark of the visor and the hot optics keep their own colours --
        // see build().
    },
});
