export type AppMenuItem =
  | {
      type?: "item";
      id: string;
      label: string;
      accelerator?: string;
      icon?: string;
      dot?: string;       // CSS color for a small dot prefix (e.g. track colors)
      enabled?: boolean;
      checked?: boolean;
      danger?: boolean;
      role?: string;
      action?: string;
      description?: string;
    }
  | {
      type: "separator";
      id: string;
    }
  | {
      type: "submenu";
      id: string;
      label: string;
      icon?: string;
      enabled?: boolean;
      children: AppMenuItem[];
    };

export type AppMenuGroup = {
  id: string;
  label: string;
  children: AppMenuItem[];
};

export const APP_MENUS: AppMenuGroup[] = [
  {
    id: "file",
    label: "File",
    children: [
      {
        id: "file.new_project",
        label: "New Project",
        accelerator: "Ctrl+N",
        icon: "file-plus",
        action: "project:new",
      },
      {
        id: "file.open_project",
        label: "Open Project...",
        accelerator: "Ctrl+O",
        icon: "folder-open",
        action: "project:open",
      },
      {
        id: "file.open_recent",
        type: "submenu",
        label: "Open Recent",
        icon: "history",
        children: [
          {
            id: "file.open_recent.empty",
            label: "No Recent Projects",
            enabled: false,
            action: "noop",
          },
          {
            id: "file.open_recent.clear",
            label: "Clear Recent Projects",
            action: "project:recent-clear",
          },
        ],
      },
      {
        type: "separator",
        id: "file.sep.save",
      },
      {
        id: "file.save",
        label: "Save",
        accelerator: "Ctrl+S",
        icon: "save",
        action: "project:save",
      },
      {
        id: "file.save_as",
        label: "Save As...",
        accelerator: "Ctrl+Shift+S",
        icon: "save-all",
        action: "project:save-as",
      },
      {
        id: "file.save_copy",
        label: "Save a Copy...",
        icon: "copy",
        action: "project:save-copy",
      },
      {
        type: "separator",
        id: "file.sep.export",
      },
      {
        id: "file.export_arrangement",
        label: "Export Arrangement...",
        accelerator: "Ctrl+E",
        icon: "download",
        action: "file:export-arrangement",
      },
      {
        type: "separator",
        id: "file.sep.close",
      },
      {
        id: "file.close_project",
        label: "Close Project",
        accelerator: "Ctrl+W",
        icon: "x",
        action: "project:close",
      },
      {
        id: "file.quit",
        label: "Quit",
        accelerator: "Alt+F4",
        icon: "power",
        role: "quit",
        action: "app:quit",
      },
    ],
  },

  {
    id: "edit",
    label: "Edit",
    children: [
      {
        id: "edit.undo",
        label: "Undo",
        accelerator: "Ctrl+Z",
        icon: "undo-2",
        action: "edit:undo",
      },
      {
        id: "edit.redo",
        label: "Redo",
        accelerator: "Ctrl+Y",
        icon: "redo-2",
        action: "edit:redo",
      },
      {
        type: "separator",
        id: "edit.sep.clipboard",
      },
      {
        id: "edit.cut",
        label: "Cut",
        accelerator: "Ctrl+X",
        icon: "scissors",
        action: "edit:cut",
      },
      {
        id: "edit.copy",
        label: "Copy",
        accelerator: "Ctrl+C",
        icon: "copy",
        action: "edit:copy",
      },
      {
        id: "edit.paste",
        label: "Paste",
        accelerator: "Ctrl+V",
        icon: "clipboard",
        action: "edit:paste",
      },
      {
        id: "edit.duplicate",
        label: "Duplicate",
        accelerator: "Ctrl+D",
        icon: "copy-plus",
        action: "edit:duplicate",
      },
      {
        id: "edit.delete",
        label: "Delete",
        accelerator: "Delete",
        icon: "trash-2",
        danger: true,
        action: "edit:delete",
      },
      {
        type: "separator",
        id: "edit.sep.select",
      },
      {
        id: "edit.select_all",
        label: "Select All",
        accelerator: "Ctrl+A",
        icon: "scan",
        action: "edit:select-all",
      },
      {
        id: "edit.deselect_all",
        label: "Deselect All",
        accelerator: "Esc",
        icon: "scan-x",
        action: "edit:deselect-all",
      },
      {
        type: "separator",
        id: "edit.sep.preferences",
      },
      {
        id: "edit.preferences",
        label: "Preferences...",
        accelerator: "Ctrl+,",
        icon: "settings",
        action: "app:preferences",
      },
    ],
  },

  {
    id: "midi",
    label: "MIDI",
    children: [
      {
        id: "midi.open_editor",
        label: "Open MIDI Editor",
        accelerator: "Ctrl+E",
        icon: "keyboard-music",
        action: "midi:open-editor",
      },
      {
        type: "separator",
        id: "midi.sep.editor",
      },
      {
        id: "midi.select_all",
        label: "Select All Notes",
        accelerator: "Ctrl+A",
        icon: "scan",
        action: "midi:select-all",
      },
      {
        id: "midi.delete_selected",
        label: "Delete Selected Notes",
        accelerator: "Delete",
        icon: "trash-2",
        danger: true,
        action: "midi:delete-selected",
      },
      {
        id: "midi.quantize",
        label: "Quantize",
        accelerator: "Q",
        icon: "align-start-vertical",
        action: "midi:quantize",
      },
      {
        id: "midi.fit_notes",
        label: "Fit Notes",
        icon: "maximize-2",
        action: "midi:fit-notes",
      },
    ],
  },

  {
    id: "project",
    label: "Project",
    children: [
      {
        id: "project.settings",
        label: "Project Settings...",
        accelerator: "Ctrl+.",
        icon: "settings-2",
        action: "project:settings",
      },
      {
        type: "separator",
        id: "project.sep.tracks",
      },
      {
        id: "project.add_audio_track",
        label: "Add Audio Track",
        icon: "mic",
        action: "track:add-audio",
      },
      {
        id: "project.add_midi_track",
        label: "Add MIDI Track",
        accelerator: "Ctrl+Shift+T",
        icon: "piano",
        action: "track:add-midi",
      },
      {
        id: "project.add_instrument_track",
        label: "Add Instrument Track",
        icon: "keyboard-music",
        action: "track:add-instrument",
      },
      {
        id: "project.add_plugin_track",
        label: "Add Plugin Track",
        icon: "cpu",
        action: "track:add-plugin",
      },
      {
        id: "project.add_bus_track",
        label: "Add Bus Track",
        icon: "route",
        action: "track:add-bus",
      },
      {
        id: "project.add_return_track",
        label: "Add Return Track",
        icon: "corner-down-left",
        action: "track:add-return",
      },
      {
        type: "separator",
        id: "project.sep.track_actions",
      },
      {
        id: "project.delete_track",
        label: "Delete Selected Track",
        icon: "trash-2",
        danger: true,
        action: "track:delete",
      },
    ],
  },

  {
    id: "audio",
    label: "Audio",
    children: [
      {
        id: "audio.play_pause",
        label: "Play / Pause",
        accelerator: "Space",
        icon: "play",
        action: "transport:play-pause",
      },
      {
        id: "audio.stop",
        label: "Stop",
        accelerator: "Shift+Space",
        icon: "square",
        action: "transport:stop",
      },
      {
        id: "audio.record",
        label: "Record",
        accelerator: "R",
        icon: "circle",
        action: "transport:record",
      },
      {
        id: "audio.loop",
        label: "Loop",
        accelerator: "L",
        icon: "repeat",
        checked: false,
        action: "transport:toggle-loop",
      },
      {
        id: "audio.metronome",
        label: "Metronome",
        accelerator: "K",
        icon: "timer",
        checked: false,
        action: "transport:toggle-metronome",
      },
      {
        type: "separator",
        id: "audio.sep.navigation",
      },
      {
        id: "audio.go_to_start",
        label: "Go to Start",
        accelerator: "Home",
        icon: "step-back",
        action: "transport:go-to-start",
      },
      {
        id: "audio.go_to_end",
        label: "Go to End",
        accelerator: "End",
        icon: "step-forward",
        action: "transport:go-to-end",
      },
      {
        id: "audio.rewind",
        label: "Rewind",
        accelerator: "Alt+Left",
        icon: "rewind",
        action: "transport:rewind",
      },
      {
        id: "audio.fast_forward",
        label: "Fast Forward",
        accelerator: "Alt+Right",
        icon: "fast-forward",
        action: "transport:fast-forward",
      },
      {
        type: "separator",
        id: "audio.sep.plugins",
      },
      {
        id: "audio.plugin_manager",
        label: "Audio Plug-in Manager...",
        icon: "blocks",
        action: "plugins:manager",
      },
    ],
  },

  {
    id: "automation",
    label: "Automation",
    children: [
      {
        id: "automation.select_all_points",
        label: "Select All Points",
        accelerator: "Ctrl+A",
        icon: "scan",
        action: "automation:select-all-points",
      },
      {
        id: "automation.delete_selected_points",
        label: "Delete Selected Points",
        accelerator: "Delete",
        icon: "trash-2",
        danger: true,
        action: "automation:delete-selected-points",
      },
      {
        id: "automation.clear_selection",
        label: "Clear Selection",
        icon: "scan-x",
        action: "automation:clear-selection",
      },
      {
        type: "separator",
        id: "automation.sep.track",
      },
      {
        id: "automation.toggle_mode",
        label: "Toggle Automation Mode",
        icon: "activity",
        action: "automation:toggle-mode",
      },
      {
        id: "automation.cycle_target",
        label: "Cycle Automation Target",
        icon: "workflow",
        action: "automation:cycle-target",
      },
    ],
  },

  {
    id: "window",
    label: "Window",
    children: [
      {
        id: "window.show_browser",
        label: "Show Browser",
        accelerator: "Ctrl+1",
        icon: "folder",
        checked: true,
        action: "panel:toggle-browser",
      },
      {
        id: "window.show_inspector",
        label: "Show Inspector",
        accelerator: "Ctrl+2",
        icon: "panel-right",
        checked: true,
        action: "panel:toggle-inspector",
      },
      {
        id: "window.show_mixer",
        label: "Show Mixer",
        accelerator: "Ctrl+3",
        icon: "sliders-horizontal",
        checked: true,
        action: "panel:toggle-mixer",
      },
      {
        id: "window.float_mixer",
        label: "Open Mixer in Window",
        icon: "external-link",
        action: "floatingwindow:mixer",
      },
      {
        type: "separator",
        id: "window.sep.zoom",
      },
      {
        id: "window.zoom_in",
        label: "Zoom In",
        accelerator: "Ctrl+=",
        icon: "zoom-in",
        action: "view:zoom-in",
      },
      {
        id: "window.zoom_out",
        label: "Zoom Out",
        accelerator: "Ctrl+-",
        icon: "zoom-out",
        action: "view:zoom-out",
      },
      {
        id: "window.reset_zoom",
        label: "Reset Zoom",
        accelerator: "Ctrl+0",
        icon: "scan",
        action: "view:reset-zoom",
      },
    ],
  },

  {
    id: "tools",
    label: "Tools",
    children: [
      {
        id: "tools.select_pointer",
        label: "Pointer Tool",
        accelerator: "V",
        icon: "pointer",
        action: "tools:select-pointer",
      },
      {
        id: "tools.select_pen",
        label: "Pen Tool",
        accelerator: "P",
        icon: "pen",
        action: "tools:select-pen",
      },
      {
        id: "tools.select_cut",
        label: "Cut Tool",
        accelerator: "C",
        icon: "scissors",
        action: "tools:select-cut",
      },
      {
        id: "tools.select_glue",
        label: "Glue Tool",
        accelerator: "G",
        icon: "combine",
        action: "tools:select-glue",
      },
      {
        id: "tools.select_time",
        label: "Time Tool",
        accelerator: "T",
        icon: "move-horizontal",
        action: "tools:select-time",
      },
      {
        id: "tools.select_automation",
        label: "Automation Tool",
        accelerator: "A",
        icon: "activity",
        action: "tools:select-automation",
      },
    ],
  },
];
