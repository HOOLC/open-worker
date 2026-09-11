// Register the background app's identity before replacing this process with
// its runtime. exec preserves PID, arguments, descriptors and signal handling.
#import <AppKit/AppKit.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <unistd.h>

int main(int argc, char **argv) {
    // A CLI compatibility symlink otherwise makes NSBundle select the outer
    // desktop app. Re-exec the canonical launcher before AppKit caches it.
    char executable[PATH_MAX], resolved[PATH_MAX];
    uint32_t size = sizeof(executable);
    if (_NSGetExecutablePath(executable, &size) != 0 || !realpath(executable, resolved)) {
        fputs("Cannot resolve Zork helper executable\n", stderr);
        return 1;
    }
    if (strcmp(executable, resolved) != 0) {
        argv[0] = resolved;
        execv(resolved, argv);
        perror("Unable to enter Zork helper bundle");
        return 1;
    }
    @autoreleasepool {
        NSBundle *bundle = NSBundle.mainBundle;
        NSString *name = [bundle objectForInfoDictionaryKey:@"ZorkRuntimeExecutable"];
        if (![name isKindOfClass:NSString.class] || name.length == 0 ||
            ![name.lastPathComponent isEqualToString:name]) {
            fputs("Missing Zork runtime executable in helper bundle\n", stderr);
            return 1;
        }
        NSString *runtime = [[bundle.bundlePath stringByAppendingPathComponent:@"Contents/MacOS"]
                             stringByAppendingPathComponent:name];
        [NSApplication sharedApplication];
        [NSApp finishLaunching];
        argv[0] = (char *)runtime.fileSystemRepresentation;
        execv(runtime.fileSystemRepresentation, argv);
        perror("Unable to launch Zork runtime");
        return 1;
    }
}
