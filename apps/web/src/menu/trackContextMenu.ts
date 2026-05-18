import type { DawTrack } from "../types/daw";
import { TRACK_COLORS } from "../theme";
import type { AppMenuItem } from "./menuItems";

const COLOR_NAMES: Record<string, string> = {
  "#56C7C9": "Cyan",
  "#7EDB9A": "Green",
  "#F2C96D": "Amber",
  "#F27E77": "Coral",
  "#A99CFF": "Violet",
  "#6EB7E8": "Blue",
  "#E89B61": "Orange",
  "#D982B6": "Rose",
  "#A8D36F": "Lime",
  "#9CAFE8": "Periwinkle",
  "#C49A6C": "Brown",
  "#71D6B5": "Mint",
};

export function buildTrackContextMenu(track: DawTrack): AppMenuItem[] {
  return [
    { id: "ctx.rename",    label: "Rename Track",    action: "track:rename" },
    {
      type: "submenu",
      id: "ctx.color",
      label: "Change Color",
      children: TRACK_COLORS.map((c, i) => ({
        id: `ctx.color.${i}`,
        label: COLOR_NAMES[c] ?? c,
        dot: c,
        action: `track:color:${c}`,
      })),
    },
    { id: "ctx.duplicate", label: "Duplicate Track", action: "track:duplicate" },
    { id: "ctx.delete",    label: "Delete Track",    danger: true, action: "edit:delete-track" },
    { type: "separator",   id: "ctx.sep1" },
    { id: "ctx.add_audio",  label: "Add Audio Track",  action: "track:add-audio" },
    { id: "ctx.add_midi",   label: "Add MIDI Track",   action: "track:add-midi",   enabled: false },
    { id: "ctx.add_plugin", label: "Add Plugin Track", action: "track:add-plugin", enabled: false },
    { id: "ctx.add_bus",    label: "Add Bus Track",    action: "track:add-bus",    enabled: false },
    { type: "separator",   id: "ctx.sep2" },
    { id: "ctx.mute",  label: track.muted  ? "Unmute" : "Mute",   action: "track:toggle-mute" },
    { id: "ctx.solo",  label: track.solo   ? "Unsolo" : "Solo",   action: "track:toggle-solo" },
    { id: "ctx.arm",   label: track.armed  ? "Disarm" : "Arm",    action: "track:toggle-arm" },
    { id: "ctx.reset_fader", label: "Reset Fader", action: "track:reset-fader" },
    { id: "ctx.reset_pan", label: "Reset Pan", action: "track:reset-pan" },
    {
      type: "submenu",
      id: "ctx.preview",
      label: "Preview Mode",
      children: [
        { id: "ctx.preview.stereo", label: "Stereo", action: "track:preview:stereo" },
        { id: "ctx.preview.mono", label: "Mono", action: "track:preview:mono" },
        { id: "ctx.preview.mid", label: "Mid", action: "track:preview:mid" },
        { id: "ctx.preview.side", label: "Side", action: "track:preview:side" },
      ],
    },
    { type: "separator",   id: "ctx.sep3" },
    { id: "ctx.add_insert", label: "Add Insert", action: "track:add-insert", enabled: false },
    { id: "ctx.add_send", label: "Add Send", action: "track:add-send", enabled: false },
    { type: "separator",   id: "ctx.sep3b" },
    { id: "ctx.freeze",   label: "Freeze Track",   action: "track:freeze",   enabled: false },
    { id: "ctx.flatten",  label: "Flatten Track",  action: "track:flatten",  enabled: false },
    { id: "ctx.route_to", label: "Route To",       action: "track:route-to", enabled: false },
    { type: "separator",  id: "ctx.sep4" },
    { id: "ctx.settings", label: "Track Settings", action: "track:settings", enabled: false },
  ];
}
