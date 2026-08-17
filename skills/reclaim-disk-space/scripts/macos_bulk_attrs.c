#include <sys/attr.h>
#include <sys/types.h>
#include <sys/vnode.h>
#include <sys/resource.h>
#include <sys/sysctl.h>
#include <sys/mount.h>
#include <mach/mach.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

#define DS_BUFFER_SIZE (1024 * 1024)
#define DS_NAME_CAPACITY 1024

uint64_t ds_free_bytes(const char *path) {
    struct statfs stats;
    if (path == NULL || statfs(path, &stats) != 0) {
        return 0;
    }
    return (uint64_t)stats.f_bavail * (uint64_t)stats.f_bsize;
}

uint32_t ds_logical_cpu_count(void) {
    uint32_t logical_cpus = 8;
    size_t value_size = sizeof(logical_cpus);
    (void)sysctlbyname("hw.logicalcpu", &logical_cpus, &value_size, NULL, 0);
    return logical_cpus > 0 ? logical_cpus : 1;
}

double ds_process_cpu_seconds(void) {
    struct rusage usage;
    if (getrusage(RUSAGE_SELF, &usage) != 0) {
        return 0.0;
    }
    return (double)usage.ru_utime.tv_sec + (double)usage.ru_utime.tv_usec / 1000000.0 +
           (double)usage.ru_stime.tv_sec + (double)usage.ru_stime.tv_usec / 1000000.0;
}

double ds_system_load_average(void) {
    double load = 0.0;
    return getloadavg(&load, 1) == 1 ? load : 0.0;
}

double ds_host_cpu_busy_fraction(void) {
    static uint64_t previous[CPU_STATE_MAX] = {0};
    static int initialized = 0;
    host_cpu_load_info_data_t load;
    mach_msg_type_number_t count = HOST_CPU_LOAD_INFO_COUNT;
    if (host_statistics(mach_host_self(), HOST_CPU_LOAD_INFO, (host_info_t)&load, &count) != KERN_SUCCESS) {
        return 0.0;
    }
    uint64_t current[CPU_STATE_MAX];
    uint64_t total = 0;
    uint64_t idle = 0;
    for (int state = 0; state < CPU_STATE_MAX; state++) {
        current[state] = load.cpu_ticks[state];
        uint64_t delta = initialized ? current[state] - previous[state] : current[state];
        total += delta;
        if (state == CPU_STATE_IDLE) { idle = delta; }
        previous[state] = current[state];
    }
    initialized = 1;
    return total > 0 ? (double)(total - idle) / (double)total : 0.0;
}

int ds_set_interactive_priority(void) {
    return setpriority(PRIO_PROCESS, 0, 10);
}

size_t ds_recommended_fd_queue_limit(void) {
    struct rlimit limit;
    if (getrlimit(RLIMIT_NOFILE, &limit) != 0 || limit.rlim_cur == RLIM_INFINITY) {
        return 8192;
    }
    size_t recommended = (size_t)(limit.rlim_cur / 4);
    if (recommended < 32) {
        recommended = (size_t)(limit.rlim_cur / 2);
    }
    if (recommended < 16) {
        recommended = 16;
    }
    if (recommended > 8192) {
        recommended = 8192;
    }
    return recommended;
}

size_t ds_recommended_worker_limit(void) {
    uint32_t logical_cpus = 8;
    uint64_t memory_bytes = 8ULL * 1024 * 1024 * 1024;
    size_t value_size = sizeof(logical_cpus);
    (void)sysctlbyname("hw.logicalcpu", &logical_cpus, &value_size, NULL, 0);
    value_size = sizeof(memory_bytes);
    (void)sysctlbyname("hw.memsize", &memory_bytes, &value_size, NULL, 0);

    size_t cpu_limit = (size_t)logical_cpus * 128;
    size_t memory_limit = (size_t)(memory_bytes / (16ULL * 1024 * 1024));
    size_t descriptor_limit = 16384;
    struct rlimit file_limit;
    if (getrlimit(RLIMIT_NOFILE, &file_limit) == 0 && file_limit.rlim_cur != RLIM_INFINITY) {
        descriptor_limit = (size_t)(file_limit.rlim_cur / 8);
    }

    size_t recommended = cpu_limit;
    if (memory_limit < recommended) { recommended = memory_limit; }
    if (descriptor_limit < recommended) { recommended = descriptor_limit; }
    if (recommended < 4) { recommended = 4; }
    if (recommended > 16384) { recommended = 16384; }
    return recommended;
}

typedef struct ds_entry {
    uint64_t file_id;
    uint64_t logical_size;
    uint64_t allocated_size;
    uint64_t private_size;
    uint64_t device_id;
    uint32_t link_count;
    uint32_t object_type;
    uint32_t error_code;
    uint32_t name_length;
    unsigned char name[DS_NAME_CAPACITY];
} ds_entry;

typedef struct ds_dir {
    int fd;
    int remaining;
    int last_errno;
    unsigned char *cursor;
    unsigned char *buffer;
} ds_dir;

static void ds_take(const unsigned char **cursor, void *destination, size_t size) {
    memcpy(destination, *cursor, size);
    *cursor += size;
}

ds_dir *ds_scanner_create(void) {
    ds_dir *directory = calloc(1, sizeof(ds_dir));
    if (directory == NULL) {
        return NULL;
    }
    directory->fd = -1;
    int allocation_error = posix_memalign((void **)&directory->buffer, 16384, DS_BUFFER_SIZE);
    if (allocation_error != 0) {
        free(directory);
        errno = allocation_error;
        return NULL;
    }
    return directory;
}

int ds_scanner_open(ds_dir *directory, const char *path) {
    if (directory == NULL || path == NULL) {
        errno = EINVAL;
        return -1;
    }
    if (directory->fd >= 0) {
        close(directory->fd);
    }
    directory->fd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    directory->remaining = 0;
    directory->last_errno = directory->fd < 0 ? errno : 0;
    directory->cursor = directory->buffer;
    return directory->fd < 0 ? -1 : 0;
}

int ds_scanner_adopt_fd(ds_dir *directory, int fd) {
    if (directory == NULL || fd < 0) {
        errno = EINVAL;
        return -1;
    }
    if (directory->fd >= 0) {
        close(directory->fd);
    }
    directory->fd = fd;
    directory->remaining = 0;
    directory->last_errno = 0;
    directory->cursor = directory->buffer;
    return 0;
}

int ds_scanner_open_child(ds_dir *directory, const char *name) {
    if (directory == NULL || directory->fd < 0 || name == NULL) {
        errno = EINVAL;
        return -1;
    }
    return openat(directory->fd, name, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
}

int ds_next_entry(ds_dir *directory, ds_entry *output) {
    if (directory == NULL || output == NULL) {
        errno = EINVAL;
        return -1;
    }

    if (directory->remaining == 0) {
        struct attrlist attributes;
        memset(&attributes, 0, sizeof(attributes));
        attributes.bitmapcount = ATTR_BIT_MAP_COUNT;
        attributes.commonattr = ATTR_CMN_RETURNED_ATTRS |
                                ATTR_CMN_ERROR |
                                ATTR_CMN_NAME |
                                ATTR_CMN_DEVID |
                                ATTR_CMN_OBJTYPE |
                                ATTR_CMN_FILEID;
        attributes.fileattr = ATTR_FILE_LINKCOUNT |
                              ATTR_FILE_TOTALSIZE |
                              ATTR_FILE_ALLOCSIZE;
        attributes.forkattr = ATTR_CMNEXT_PRIVATESIZE;

        int count = getattrlistbulk(
            directory->fd,
            &attributes,
            directory->buffer,
            DS_BUFFER_SIZE,
            FSOPT_PACK_INVAL_ATTRS | FSOPT_ATTR_CMN_EXTENDED
        );
        if (count <= 0) {
            directory->last_errno = count < 0 ? errno : 0;
            return count;
        }
        directory->remaining = count;
        directory->cursor = directory->buffer;
    }

    memset(output, 0, sizeof(*output));
    const unsigned char *group = directory->cursor;
    const unsigned char *cursor = group;
    uint32_t group_length = 0;
    attribute_set_t returned;
    attrreference_t name_reference;
    dev_t device = 0;
    fsobj_type_t object_type = 0;
    off_t logical_size = 0;
    off_t allocated_size = 0;
    off_t private_size = 0;

    ds_take(&cursor, &group_length, sizeof(group_length));
    if (group_length < sizeof(uint32_t) + sizeof(attribute_set_t) ||
        group_length > DS_BUFFER_SIZE ||
        group + group_length > directory->buffer + DS_BUFFER_SIZE) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    directory->cursor += group_length;
    directory->remaining -= 1;

    ds_take(&cursor, &returned, sizeof(returned));
    ds_take(&cursor, &output->error_code, sizeof(output->error_code));
    const unsigned char *name_reference_location = cursor;
    ds_take(&cursor, &name_reference, sizeof(name_reference));
    ds_take(&cursor, &device, sizeof(device));
    ds_take(&cursor, &object_type, sizeof(object_type));
    ds_take(&cursor, &output->file_id, sizeof(output->file_id));
    if (returned.fileattr & ATTR_FILE_LINKCOUNT) {
        ds_take(&cursor, &output->link_count, sizeof(output->link_count));
    }
    if (returned.fileattr & ATTR_FILE_TOTALSIZE) {
        ds_take(&cursor, &logical_size, sizeof(logical_size));
    }
    if (returned.fileattr & ATTR_FILE_ALLOCSIZE) {
        ds_take(&cursor, &allocated_size, sizeof(allocated_size));
    }
    if (returned.forkattr & ATTR_CMNEXT_PRIVATESIZE) {
        ds_take(&cursor, &private_size, sizeof(private_size));
    }

    const unsigned char *name = name_reference_location +
                                name_reference.attr_dataoffset;
    size_t name_length = name_reference.attr_length;
    if (name_length > 0 && name[name_length - 1] == '\0') {
        name_length -= 1;
    }
    if (name < group || name + name_length > group + group_length) {
        output->error_code = EIO;
        name_length = 0;
    }
    if (name_length >= DS_NAME_CAPACITY) {
        name_length = DS_NAME_CAPACITY - 1;
    }
    memcpy(output->name, name, name_length);
    output->name_length = (uint32_t)name_length;
    output->device_id = (uint64_t)device;
    output->object_type = (uint32_t)object_type;

    if (returned.fileattr & ATTR_FILE_TOTALSIZE) {
        output->logical_size = logical_size > 0 ? (uint64_t)logical_size : 0;
    }
    if (returned.fileattr & ATTR_FILE_ALLOCSIZE) {
        output->allocated_size = allocated_size > 0 ? (uint64_t)allocated_size : 0;
    }
    if (returned.forkattr & ATTR_CMNEXT_PRIVATESIZE) {
        output->private_size = private_size > 0 ? (uint64_t)private_size : 0;
    }
    return 1;
}

int ds_last_errno(ds_dir *directory) {
    return directory == NULL ? errno : directory->last_errno;
}

void ds_scanner_close(ds_dir *directory) {
    if (directory == NULL) {
        return;
    }
    if (directory->fd >= 0) {
        close(directory->fd);
        directory->fd = -1;
    }
    directory->remaining = 0;
}

void ds_scanner_destroy(ds_dir *directory) {
    if (directory == NULL) {
        return;
    }
    ds_scanner_close(directory);
    free(directory->buffer);
    free(directory);
}
