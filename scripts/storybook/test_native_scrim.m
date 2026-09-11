#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <dlfcn.h>
#include <assert.h>
extern void *zork_modal_blur_create(void *);
extern void zork_modal_blur_update(void *,double,double,double,double,double);
extern void zork_modal_blur_destroy(void *);
@interface ScrimTestCanvas : NSView @end
@implementation ScrimTestCanvas
- (BOOL)isFlipped { return YES; }
@end
static void tick(double seconds) {
    NSDate *end = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (end.timeIntervalSinceNow > 0) [[NSRunLoop mainRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.005]];
}
static float opacity(NSView *view) {return view.layer.presentationLayer ? view.layer.presentationLayer.opacity : view.layer.opacity;}
static void check(BOOL condition, const char *message) {
    if (!condition) { fprintf(stderr,"FAIL %s\n",message); exit(1); }
}
static void coverage(NSWindow *window, NSView *source, NSView *scrim) {
    NSView *host=window.contentView.superview;
    check(scrim.superview==host && host.subviews.lastObject==scrim,"titlebar must be below the scrim");
    check(NSEqualRects(scrim.frame,host.bounds),"scrim must cover the full window");
    check(fabs(CGColorGetAlpha(scrim.layer.backgroundColor)-0.35)<0.001,"scrim should be 35 percent opaque");
    CAShapeLayer *mask=(CAShapeLayer *)scrim.layer.mask;
    check(NSEqualRects(mask.frame,scrim.bounds),"mask must follow window resizing");
    check(CGPathContainsPoint(mask.path,NULL,CGPointMake(2,2),true),"window top edge must be shaded");
    check(CGPathContainsPoint(mask.path,NULL,CGPointMake(NSWidth(scrim.bounds)-2,NSHeight(scrim.bounds)-2),true),"window bottom edge must be shaded");
    NSPoint center=[scrim convertPoint:NSMakePoint(240,160) fromView:source];
    check(!CGPathContainsPoint(mask.path,NULL,center,true),"foreground card must stay clear");
}
static void capture(NSWindow *window) {
    const char *path=getenv("ZORK_SCRIM_CAPTURE"); if(!path)return;
    typedef CGImageRef (*Capture)(CGRect,CGWindowListOption,CGWindowID,CGWindowImageOption);
    Capture take=(Capture)dlsym(RTLD_DEFAULT,"CGWindowListCreateImage");
    CGImageRef image=take?take(CGRectNull,kCGWindowListOptionIncludingWindow,(CGWindowID)window.windowNumber,kCGWindowImageBoundsIgnoreFraming):NULL;
    if(image) {
        NSBitmapImageRep *bitmap=[[NSBitmapImageRep alloc] initWithCGImage:image];
        [[bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:[NSString stringWithUTF8String:path] atomically:YES];
        CGImageRelease(image);
    }
}
int main() { @autoreleasepool {
    [NSApplication sharedApplication]; [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory]; [NSApp finishLaunching];
    for (int full=0;full<2;full++) {
        NSWindowStyleMask style=NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable;
        if(full)style|=NSWindowStyleMaskFullSizeContentView;
        NSWindow *window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,480,300) styleMask:style backing:NSBackingStoreBuffered defer:NO];
        window.releasedWhenClosed=NO; window.titlebarAppearsTransparent=YES; window.titleVisibility=NSWindowTitleHidden;
        ScrimTestCanvas *source=[[ScrimTestCanvas alloc] initWithFrame:window.contentView.bounds];
        source.wantsLayer=YES; source.layer.backgroundColor=NSColor.whiteColor.CGColor; window.contentView=source;
        [window orderFront:nil]; tick(0.1);
        NSView *host=window.contentView.superview; NSUInteger originalCount=host.subviews.count;
        void *handle=zork_modal_blur_create((__bridge void *)source); check(handle!=NULL,"native scrim available");
        NSView *scrim=(__bridge NSView *)handle;
        zork_modal_blur_update(handle,100,80,280,160,32); [CATransaction flush]; tick(0.07);
        float enter=opacity(scrim); tick(0.25); check(opacity(scrim)>0.99,"entry endpoint");
        coverage(window,source,scrim);
        if(full)capture(window);
        [window setContentSize:NSMakeSize(640,420)]; tick(0.05);
        zork_modal_blur_update(handle,100,80,280,160,32); coverage(window,source,scrim);
        zork_modal_blur_destroy(handle); [CATransaction flush]; tick(0.05);
        float exitOpacity=opacity(scrim);
        void *reopened=zork_modal_blur_create((__bridge void *)source); check(reopened==(__bridge void *)scrim,"rapid reopen should reuse scrim");
        zork_modal_blur_update(reopened,100,80,280,160,32); [CATransaction flush];
        float reverse=opacity(scrim); tick(0.3); check(opacity(scrim)>0.99,"reopen endpoint"); coverage(window,source,scrim);
        if (!NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion) {
            check(enter>0 && enter<1,"entry animation"); check(exitOpacity>0 && exitOpacity<1,"exit animation"); check(fabsf(reverse-exitOpacity)<0.15,"continuous reversal");
        }
        zork_modal_blur_destroy(reopened); tick(0.3); check(host.subviews.count==originalCount,"scrim cleanup");
        printf("PASS native compositor: full_size=%d, full-window coverage, resize, clear foreground, opacity 0.35; enter %.3f exit %.3f reverse %.3f; cleanup complete\n",full,enter,exitOpacity,reverse);
        [window close];
    }
} return 0; }
