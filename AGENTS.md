@CLAUDE.md

## Cursor Cloud specific instructions

Scope note: this environment is set up for the **native GPUI app only**
(`apps/native/studio`, binary `FutureboardNative`, package `futureboard_native`).
The Web (WASM) surface was intentionally left out of scope.

Standard build/run/lint/test commands live in `README.md` and `CONTRIBUTING.md`
(and `CLAUDE.md` §6). Below are only the non-obvious, durable caveats for this
headless Linux VM (no GPU, no sound card). The update script already handles
submodule init + `rustup default stable`; system packages are baked into the
snapshot.

### Building a *runnable* native app (important gotcha)
`cargo build -p futureboard_native` (a.k.a. `bun run dev:native` /
`cargo run -p futureboard_native`) builds only the main binary. At runtime the
app **requires a sibling helper binary** `FutureboardPluginHostX64` next to the
main executable — the external plugin bridge is mandatory and the app aborts
project/session load (`"could not be restored into the session"`) if it is
missing. Build it too:

```
cargo build -p futureboard_native
cargo build -p sphere-plugin-host --bins   # -> target/debug/FutureboardPluginHostX64 (+ FutureboardPluginScanner)
```

`cargo build --workspace` also produces both. First native build is heavy (GPUI
+ vendored VST3/CLAP C++). The C++ bridge needs `cc`/`c++` = GCC; the base image
defaults `c++` to clang which cannot find libstdc++ headers, so the snapshot
pins the alternatives to gcc/g++ (`update-alternatives --set c++ /usr/bin/g++`).

### Running the GUI (no GPU + must use Wayland)
GPUI's renderer includes the wgpu GL backend, whose surface creation is
unimplemented for X11/xcb and **panics on X11** (`not yet implemented: xcb`).
So the app must run under a **Wayland** compositor. There is no GPU; Mesa
software drivers (lavapipe Vulkan / llvmpipe GL) are used.

Recipe (a nested Weston window shows up on the X11 desktop `:1` so it is visible
to screen capture):

```
export XDG_RUNTIME_DIR=/tmp/xdg-runtime; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR
# 1) start EXACTLY ONE nested compositor (multiple instances on the same socket cause flakiness)
DISPLAY=:1 weston --backend=x11 --width=1680 --height=1010 --socket=wayland-fb &
# 2) start audio (see below), then run the app against Wayland with software GL:
env -u DISPLAY WAYLAND_DISPLAY=wayland-fb XDG_RUNTIME_DIR=/tmp/xdg-runtime \
  LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe MESA_LOADER_DRIVER_OVERRIDE=llvmpipe \
  cargo run -p futureboard_native
```

### Audio device is required for session load
There is no `/dev/snd` and kernel modules cannot be loaded. The engine's
`build_and_warm_audio_engine` returns `Err` (aborting session/studio load) when
no output device exists. A PulseAudio null sink is used as a virtual device and
`/etc/asound.conf` routes ALSA `default` → PulseAudio. Start it before running:

```
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
pulseaudio -D --exit-idle-time=-1 --disallow-exit
pactl load-module module-null-sink sink_name=vspeaker
pactl set-default-sink vspeaker
```

Then cpal/ALSA reports `pulse` + `default` output devices and the studio loads.
MIDI stays unavailable (`/dev/snd/seq` missing) — that is expected and harmless.

### Software-rendering caveats (durable)
- A newly opened GPUI window (e.g. the studio window) can paint **black until it
  receives an input event** — click or resize the window once to force the first
  paint.
- The Welcome→Studio transition (which opens a transient "loading session"
  window) is **flaky** under the nested software-rendered compositor and can make
  the process exit cleanly mid-transition; just relaunch until it reaches
  `[SessionLoad] studio ready`. Setting the app config `general.show_start_screen`
  toggles booting straight into an empty workspace vs. the Welcome hub.
- Set `FUTUREBOARD_BOOT_DEBUG=1` for boot/lifecycle logs; `[SessionLoad] studio ready`
  means the workspace mounted.
