// Static WASM imports: no evaluation of JavaScript source supplied as strings.
mergeInto(LibraryManager.library, {
  kapsule_beep: function () { globalThis.kapsuleAudio?.beep(); },
  kapsule_audio_play__deps: ['$UTF8ToString'],
  kapsule_audio_play: function (path) { return globalThis.kapsuleAudio?.play(UTF8ToString(path)) ? 1 : 0; },
  kapsule_audio_stop: function () { globalThis.kapsuleAudio?.stop(); },
  kapsule_audio_playing: function () { return globalThis.kapsuleAudio?.playing() ? 1 : 0; },
  kapsule_dialog__deps: ['$UTF8ToString', '$lengthBytesUTF8', '$stringToUTF8', 'malloc'],
  kapsule_dialog: function (kind, prompt, fallback) {
    if (!globalThis.kapsuleDialog) return 0;
    const response = JSON.stringify(globalThis.kapsuleDialog(UTF8ToString(kind), UTF8ToString(prompt), JSON.parse(UTF8ToString(fallback))));
    const length = lengthBytesUTF8(response) + 1;
    if (length > 65536) return 0;
    const pointer = _malloc(length);
    if (!pointer) return 0;
    stringToUTF8(response, pointer, length);
    return pointer;
  },
});
