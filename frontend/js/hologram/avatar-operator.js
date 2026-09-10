/* Avatar: Operator -- a terminal prompt sitting in the mouth of a tunnel of falling
 * code. Two ideas, borrowed as pure technique rather than as any specific film's
 * artwork: a caret that blinks at the end of a line perpetually typing itself out, and
 * a rushing cylinder of digit glyphs receding into the distance, the classic "camera
 * dives through the code" trick that a perspective camera gives you for free once the
 * glyphs sit on a constant-radius tube -- near ones spread wide, far ones crowd toward
 * a vanishing point at the centre.
 *
 * No obsidian core, no fixed hot accent: like A1 and White Rabbit, this reads as a
 * projection rather than a physical creature, so it's fully theme-tinted throughout.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

const OPERATOR_TUNNEL_NEAR_Z = 80;
const OPERATOR_TUNNEL_FAR_Z = -170;
const OPERATOR_TUNNEL_DEPTH = OPERATOR_TUNNEL_NEAR_Z - OPERATOR_TUNNEL_FAR_Z;

// Draws a single character onto a transparent square canvas for use as a point-sprite
// map -- the same idiom as the built-in glow textures, just with a glyph baked in
// instead of a gradient.
function operatorGlyphTexture(char, size) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    ctx.font = `bold ${Math.floor(size * 0.8)}px "Courier New", Consolas, monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = '#ffffff';
    ctx.fillText(char, size / 2, size / 2 + size * 0.05);
    return new THREE.CanvasTexture(canvas);
}

// One "stream" of glyphs scattered through the tunnel, each carrying its own fixed
// radius/angle/speed so the whole cloud reads as falling code rather than a rigid
// lattice. Every particle's path is a pure function of ctx.time (see updateOperator-
// Stream) -- nothing here is integrated frame to frame, so there's no per-frame state
// to drift or need resetting.
function buildOperatorStream(texture, count, color, pointSize, baseOpacity) {
    const positions = new Float32Array(count * 3);
    const colors = new Float32Array(count * 3);
    const state = new Array(count);
    for (let i = 0; i < count; i++) {
        const brightness = 0.75 + Math.random() * 0.55;
        state[i] = {
            angle0: Math.random() * Math.PI * 2,
            radius: 3 + Math.random() * 55,
            speed: 16 + Math.random() * 26,
            offset: Math.random() * OPERATOR_TUNNEL_DEPTH,
            spin: (Math.random() - 0.5) * 0.5,
            nextFlicker: Math.random() * 2,
            brightness,
        };
        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = brightness;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    const material = new THREE.PointsMaterial({
        color, map: texture, size: pointSize, vertexColors: true,
        transparent: true, opacity: baseOpacity, depthWrite: false,
        blending: THREE.AdditiveBlending, sizeAttenuation: true,
    });
    const points = new THREE.Points(geometry, material);
    return { points, material, positions, colors, state };
}

// Rewrites one stream's positions/brightness for this frame. speedMult scales both the
// fall rate and the swirl rate together, so a faster stream also spins faster -- reads
// as more urgent rather than just quicker, the same way the tentacle avatars speed up
// their whole gait rather than just their frequency.
function updateOperatorStream(stream, ctx, speedMult) {
    const { positions, colors, state } = stream;
    for (let i = 0; i < state.length; i++) {
        const s = state[i];
        const flow = (ctx.time * s.speed * speedMult + s.offset) % OPERATOR_TUNNEL_DEPTH;
        const z = OPERATOR_TUNNEL_FAR_Z + flow;
        const angle = s.angle0 + ctx.time * s.spin * speedMult;

        if (ctx.time > s.nextFlicker) {
            s.brightness = 0.65 + Math.random() * 0.85;
            s.nextFlicker = ctx.time + 0.15 + Math.random() * 1.1;
        }

        positions[i * 3] = Math.cos(angle) * s.radius;
        positions[i * 3 + 1] = Math.sin(angle) * s.radius;
        positions[i * 3 + 2] = z;
        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = s.brightness;
    }
    stream.points.geometry.attributes.position.needsUpdate = true;
    stream.points.geometry.attributes.color.needsUpdate = true;
}

HologramAvatar.registerAvatar({
    id: 'operator',
    label: 'Operator',

    build(api) {
        const group = new THREE.Group();

        const zeroTexture = operatorGlyphTexture('0', 64);
        const oneTexture = operatorGlyphTexture('1', 64);

        // The main fall: mostly "0"s, the densest and brightest stream.
        const zeros = buildOperatorStream(zeroTexture, 1500, api.palette.hex, 9, 1.0);
        // A sparser accent stream of "1"s threaded through it, a shade dimmer -- the
        // two-glyph mix a digital rain needs to read as code rather than static.
        const ones = buildOperatorStream(oneTexture, 480, api.palette.hex2, 7.2, 0.8);
        group.add(zeros.points, ones.points);

        // The prompt: a row of ticks that perpetually "types" itself out left to right,
        // and a caret that blinks at wherever the line currently ends -- the waiting
        // terminal the tunnel of code rushes past.
        const promptCount = 20;
        const promptSpacing = 2.3;
        const promptStartX = -((promptCount - 1) * promptSpacing) / 2;
        const promptPositions = new Float32Array(promptCount * 3);
        const promptColors = new Float32Array(promptCount * 3);
        const promptZ = OPERATOR_TUNNEL_NEAR_Z + 18;
        for (let i = 0; i < promptCount; i++) {
            promptPositions[i * 3] = promptStartX + i * promptSpacing;
            promptPositions[i * 3 + 1] = -2;
            promptPositions[i * 3 + 2] = promptZ;
        }
        const promptGeometry = new THREE.BufferGeometry();
        promptGeometry.setAttribute('position', new THREE.BufferAttribute(promptPositions, 3));
        promptGeometry.setAttribute('color', new THREE.BufferAttribute(promptColors, 3));
        const promptMaterial = new THREE.PointsMaterial({
            color: api.palette.hex3, map: zeroTexture, size: 5.5, vertexColors: true,
            transparent: true, opacity: 0.95, depthWrite: false,
            blending: THREE.AdditiveBlending, sizeAttenuation: true,
        });
        const promptPoints = new THREE.Points(promptGeometry, promptMaterial);
        group.add(promptPoints);

        const cursorMaterial = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.9, depthWrite: false,
        });
        const cursor = new THREE.Mesh(new THREE.PlaneGeometry(3.6, 11), cursorMaterial);
        cursor.position.set(promptStartX, -2, promptZ);
        group.add(cursor);

        // Soft ambient glow behind everything, theme-tinted like the rest.
        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true,
            opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(170, 170, 1);
        glow.position.z = -40;
        group.add(glow);

        return {
            group, zeros, ones,
            promptPoints, promptMaterial, promptPositions, promptColors,
            promptCount, promptStartX, promptSpacing,
            cursor, cursorMaterial, glow, glowMaterial,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isListening = ctx.state === 'LISTENING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // The tunnel never stops flowing -- like the tentacle avatars' constant
        // swimming, this is meant to always read as "live data," just calmer at rest.
        let speedMult = 1;
        if (isThinking) speedMult = 2.4;
        else if (isListening) speedMult = 1.5;
        else if (isSpeaking) speedMult = 1.15 + ctx.audio * 0.9;
        speedMult += ctx.click * 1.4;

        updateOperatorStream(model.zeros, ctx, speedMult);
        updateOperatorStream(model.ones, ctx, speedMult);

        // A gentle overall wobble so the tunnel doesn't feel like a static tube.
        model.group.rotation.z = Math.sin(ctx.time * 0.15) * 0.03;
        model.group.rotation.x = Math.sin(ctx.time * 0.11) * 0.02 + ctx.click * 0.05;

        // The prompt line "types" itself out over a repeating cycle, faster while
        // thinking -- the terminal actively working rather than idling.
        const period = isThinking ? 1.8 : 3.6;
        const cyclePos = (ctx.time % period) / period;
        const leadIndex = cyclePos * (model.promptCount - 1);
        for (let i = 0; i < model.promptCount; i++) {
            const dist = leadIndex - i;
            let brightness;
            if (dist < 0) brightness = 0.03; // not typed yet
            else if (dist < 1.2) brightness = 1.0; // the leading edge, writing now
            else brightness = 0.3 + (isSpeaking ? ctx.audio * 0.3 : 0); // already printed
            model.promptColors[i * 3] = model.promptColors[i * 3 + 1] = model.promptColors[i * 3 + 2] = brightness;
        }
        model.promptPoints.geometry.attributes.color.needsUpdate = true;
        model.cursor.position.x = model.promptStartX + leadIndex * model.promptSpacing;

        // The caret blinks while waiting, and holds a steady voice-reactive glow while
        // actually speaking -- it stops "waiting" once it has something to say.
        let cursorOpacity;
        if (isSpeaking) {
            cursorOpacity = 0.6 + ctx.audio * 0.4;
        } else {
            const blinkRate = isThinking ? 4.5 : 1.4;
            cursorOpacity = Math.floor(ctx.time * blinkRate) % 2 === 0 ? 0.95 : 0.15;
        }
        model.cursorMaterial.opacity = cursorOpacity;

        let glowIntensity;
        if (isSpeaking) glowIntensity = 0.14 + ctx.audio * 0.3;
        else if (isThinking) glowIntensity = 0.14 + Math.abs(Math.sin(ctx.time * 6)) * 0.12;
        else glowIntensity = 0.12 + ctx.click * 0.2;
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.zeros.material.color.setHex(palette.hex);
        model.ones.material.color.setHex(palette.hex2);
        model.promptMaterial.color.setHex(palette.hex3);
        model.cursorMaterial.color.setHex(palette.hex);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
