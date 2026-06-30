#include "editor_mac_internal.hpp"

@implementation DauxEditorWindowDelegate

- (BOOL)windowShouldClose:(NSWindow *)sender {
  (void)sender;
  SphereDauxVst3Processor *proc = self.processor;
  if (proc) {
    // Detach + release — this zeroes editor_native_window so re-entrant
    // calls from windowWillClose: or any queued callbacks are no-ops.
    close_editor_mac(proc);
  }
  return NO; // we handle the close ourselves via close_editor_mac
}

@end
