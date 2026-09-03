// Build-time equivalent of the `tailwind.config` that used to sit inline in index.html
// next to the Tailwind CDN script. The CDN build compiled styles in the browser on every
// launch; this config feeds the same theme to a one-off build that produces
// frontend/vendor/tailwind.css. See scripts/build_vendor_css.sh.
module.exports = {
    content: [
        'frontend/*.html',
        'frontend/js/**/*.js',
    ],
    theme: {
        extend: {
            colors: {
                cyber: {
                    cyan: 'var(--neon-cyan)',
                    blue: 'var(--neon-blue)',
                    purple: 'var(--neon-purple)',
                    green: 'var(--neon-green)',
                    amber: 'var(--neon-amber)',
                    dark: 'var(--bg-core)',
                    panel: 'var(--bg-panel)'
                }
            },
            fontFamily: {
                orbitron: ['Orbitron', 'sans-serif'],
                rajdhani: ['Rajdhani', 'sans-serif'],
                mono: ['"Share Tech Mono"', 'monospace']
            }
        }
    }
};
