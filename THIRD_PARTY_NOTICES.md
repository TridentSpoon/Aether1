This offline installer bundles pre-built binaries and models from other projects so it
needs no internet access at install time. None of this is AETHER1's own code; each
component is licensed by its own project under its own terms.

## Piper (text-to-speech)

- Project: https://github.com/rhasspy/piper
- License: MIT

### Bundled voice: `en_US-lessac-medium`

- Source: https://huggingface.co/rhasspy/piper-voices
- Voice data license: see the voice card on the above page (Piper voices are typically
  released under permissive or public-domain-equivalent terms by their individual
  contributors; check the specific voice's card for its exact license before redistributing
  this bundle further).

## whisper.cpp (speech-to-text)

- Project: https://github.com/ggml-org/whisper.cpp
- License: MIT

### Bundled model: `ggml-small.bin`

- Source: https://huggingface.co/ggerganov/whisper.cpp
- This is a format conversion of OpenAI's Whisper model weights.
- Whisper model license: MIT (see https://github.com/openai/whisper)

## espeak-ng (fallback text-to-speech, Linux)

Not bundled in this installer -- installed separately via your distribution's package
manager (see `setup.sh`) or, on Windows, replaced by the built-in SAPI voice instead.

- Project: https://github.com/espeak-ng/espeak-ng
- License: GPL-3.0
