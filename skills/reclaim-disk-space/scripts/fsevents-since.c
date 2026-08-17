#include <CoreServices/CoreServices.h>
#include <dispatch/dispatch.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct event_context {
    dispatch_semaphore_t history_done;
} event_context;

static void print_escaped_path(const char *path) {
    const unsigned char *cursor = (const unsigned char *)path;
    while (*cursor != '\0') {
        unsigned char value = *cursor++;
        if (value == '%' || value == '\t' || value == '\n' || value == '\r' || value < 0x20) {
            printf("%%%02X", value);
        } else {
            putchar(value);
        }
    }
}

static void event_callback(
    ConstFSEventStreamRef stream,
    void *context_pointer,
    size_t event_count,
    void *event_paths,
    const FSEventStreamEventFlags event_flags[],
    const FSEventStreamEventId event_ids[]
) {
    (void)stream;
    event_context *context = (event_context *)context_pointer;
    CFArrayRef paths = (CFArrayRef)event_paths;
    const FSEventStreamEventFlags reset_flags =
        kFSEventStreamEventFlagMustScanSubDirs |
        kFSEventStreamEventFlagUserDropped |
        kFSEventStreamEventFlagKernelDropped |
        kFSEventStreamEventFlagEventIdsWrapped |
        kFSEventStreamEventFlagRootChanged;

    for (size_t index = 0; index < event_count; index++) {
        FSEventStreamEventFlags flags = event_flags[index];
        if (flags & kFSEventStreamEventFlagHistoryDone) {
            dispatch_semaphore_signal(context->history_done);
            continue;
        }

        CFStringRef path_value = (CFStringRef)CFArrayGetValueAtIndex(paths, (CFIndex)index);
        char path[PATH_MAX];
        if (!CFStringGetFileSystemRepresentation(path_value, path, sizeof(path))) {
            strcpy(path, "<unrepresentable-path>");
        }

        printf("%s\t%llu\t0x%08x\t",
               (flags & reset_flags) ? "RESET" : "EVENT",
               (unsigned long long)event_ids[index],
               (unsigned int)flags);
        print_escaped_path(path);
        putchar('\n');
    }
    fflush(stdout);
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--current") == 0) {
        printf("%llu\n", (unsigned long long)FSEventsGetCurrentEventId());
        return 0;
    }
    if (argc != 3) {
        fprintf(stderr, "usage: fsevents-since --current | ROOT SINCE_EVENT_ID\n");
        return 2;
    }

    char *end = NULL;
    unsigned long long since_value = strtoull(argv[2], &end, 10);
    if (end == argv[2] || *end != '\0') {
        fprintf(stderr, "invalid event id: %s\n", argv[2]);
        return 2;
    }

    CFStringRef root = CFStringCreateWithFileSystemRepresentation(kCFAllocatorDefault, argv[1]);
    if (root == NULL) {
        fprintf(stderr, "unable to represent root path\n");
        return 1;
    }
    const void *root_values[] = {root};
    CFArrayRef roots = CFArrayCreate(kCFAllocatorDefault, root_values, 1, &kCFTypeArrayCallBacks);
    event_context context = { .history_done = dispatch_semaphore_create(0) };
    FSEventStreamContext stream_context = {0, &context, NULL, NULL, NULL};
    FSEventStreamRef stream = FSEventStreamCreate(
        kCFAllocatorDefault,
        event_callback,
        &stream_context,
        roots,
        (FSEventStreamEventId)since_value,
        0.05,
        kFSEventStreamCreateFlagUseCFTypes |
        kFSEventStreamCreateFlagNoDefer |
        kFSEventStreamCreateFlagWatchRoot
    );
    if (stream == NULL) {
        fprintf(stderr, "unable to create FSEvent stream\n");
        CFRelease(roots);
        CFRelease(root);
        return 1;
    }

    dispatch_queue_t queue = dispatch_queue_create("reclaim-disk-space.fsevents", DISPATCH_QUEUE_SERIAL);
    FSEventStreamSetDispatchQueue(stream, queue);
    if (!FSEventStreamStart(stream)) {
        fprintf(stderr, "unable to start FSEvent stream\n");
        FSEventStreamInvalidate(stream);
        FSEventStreamRelease(stream);
        CFRelease(roots);
        CFRelease(root);
        return 1;
    }

    long wait_result = dispatch_semaphore_wait(
        context.history_done,
        dispatch_time(DISPATCH_TIME_NOW, 30LL * NSEC_PER_SEC)
    );
    FSEventStreamStop(stream);
    FSEventStreamInvalidate(stream);
    FSEventStreamRelease(stream);
    CFRelease(roots);
    CFRelease(root);

    if (wait_result != 0) {
        fprintf(stderr, "timed out waiting for FSEvent history\n");
        return 1;
    }
    printf("CURRENT\t%llu\n", (unsigned long long)FSEventsGetCurrentEventId());
    return 0;
}
