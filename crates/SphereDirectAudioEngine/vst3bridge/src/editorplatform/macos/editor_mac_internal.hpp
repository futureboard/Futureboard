#pragma once

#import <Cocoa/Cocoa.h>

#include "../../../include/sphere_daux_editor_bridge.h"

NSColor *daux_bg_color(void);

void close_editor_mac(SphereDauxVst3Processor *proc);

/// Receives close-button clicks from the NSWindow and delegates them to the
/// processor's close path so IPlugView::removed() is called correctly.
@interface DauxEditorWindowDelegate : NSObject <NSWindowDelegate>
@property(nonatomic, assign) SphereDauxVst3Processor *processor;
@end
