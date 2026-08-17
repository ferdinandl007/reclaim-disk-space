#include <sys/attr.h>
#include <sys/types.h>
#include <sys/vnode.h>
#include <sys/resource.h>
#include <sys/sysctl.h>
#include <sys/mount.h>
#include <sys/stat.h>
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

int ds_open_directory_fd(const char *path) {
    if (path == NULL) {
        errno = EINVAL;
        return -1;
    }
    return open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
}

static int ds_open_relative_parent(int root_fd, const char *relative, int *parent_fd, char *leaf, size_t leaf_size) {
    if (root_fd < 0 || relative == NULL || parent_fd == NULL || leaf == NULL || leaf_size == 0 || relative[0] == '/') {
        errno = EINVAL;
        return -1;
    }
    char *copy = strdup(relative);
    if (copy == NULL) {
        return -1;
    }
    char *last_separator = strrchr(copy, '/');
    char *parent_path = copy;
    if (last_separator != NULL) {
        *last_separator = '\0';
        if (strlcpy(leaf, last_separator + 1, leaf_size) >= leaf_size) {
            free(copy);
            errno = ENAMETOOLONG;
            return -1;
        }
    } else {
        if (strlcpy(leaf, copy, leaf_size) >= leaf_size) {
            free(copy);
            errno = ENAMETOOLONG;
            return -1;
        }
        parent_path = copy + strlen(copy);
    }
    if (leaf[0] == '\0') {
        free(copy);
        errno = EINVAL;
        return -1;
    }

    int current = dup(root_fd);
    if (current < 0) {
        free(copy);
        return -1;
    }
    int flags = O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW;
    char *cursor = parent_path;
    char *component = NULL;
    while ((component = strsep(&cursor, "/")) != NULL) {
        if (component[0] == '\0' || strcmp(component, ".") == 0) {
            continue;
        }
        if (strcmp(component, "..") == 0) {
            close(current);
            free(copy);
            errno = EINVAL;
            return -1;
        }
        int next = openat(current, component, flags);
        if (next < 0) {
            close(current);
            free(copy);
            return -1;
        }
        close(current);
        current = next;
    }
    free(copy);
    *parent_fd = current;
    return 0;
}

static int ds_remove_relative(int root_fd, const char *relative, uint64_t expected_device, uint64_t expected_inode, uint32_t expected_mode, int directory) {
    if (relative == NULL || relative[0] == '\0') {
        errno = EINVAL;
        return -1;
    }
    char leaf[DS_NAME_CAPACITY];
    int parent_fd = -1;
    if (ds_open_relative_parent(root_fd, relative, &parent_fd, leaf, sizeof(leaf)) != 0) {
        return -1;
    }
    struct stat metadata;
    if (fstatat(parent_fd, leaf, &metadata, AT_SYMLINK_NOFOLLOW) != 0) {
        int saved = errno;
        close(parent_fd);
        errno = saved;
        return -1;
    }
    if ((uint64_t)metadata.st_dev != expected_device || (uint64_t)metadata.st_ino != expected_inode ||
        ((uint32_t)metadata.st_mode & S_IFMT) != (expected_mode & S_IFMT)) {
        close(parent_fd);
        errno = EAGAIN;
        return -1;
    }
    int result = unlinkat(parent_fd, leaf, directory ? AT_REMOVEDIR : 0);
    int saved = errno;
    close(parent_fd);
    errno = saved;
    return result;
}

int ds_unlink_relative(int root_fd, const char *relative, uint64_t expected_device, uint64_t expected_inode, uint32_t expected_mode) {
    return ds_remove_relative(root_fd, relative, expected_device, expected_inode, expected_mode, 0);
}

int ds_remove_directory_relative(int root_fd, const char *relative, uint64_t expected_device, uint64_t expected_inode, uint32_t expected_mode) {
    return ds_remove_relative(root_fd, relative, expected_device, expected_inode, expected_mode, 1);
}

void ds_close_fd(int fd) {
    if (fd >= 0) {
        close(fd);
    }
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

static int ds_take(const unsigned char **cursor, const unsigned char *end, void *destination, size_t size) {
    if (cursor == NULL || *cursor == NULL || end == NULL || *cursor > end || size > (size_t)(end - *cursor)) {
        return -1;
    }
    memcpy(destination, *cursor, size);
    *cursor += size;
    return 0;
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

int ds_scanner_child_times(ds_dir *directory, const char *name, uint64_t *created_seconds, uint64_t *modified_seconds) {
    if (directory == NULL || directory->fd < 0 || name == NULL || created_seconds == NULL || modified_seconds == NULL) {
        errno = EINVAL;
        return -1;
    }
    struct stat metadata;
    if (fstatat(directory->fd, name, &metadata, AT_SYMLINK_NOFOLLOW) != 0) {
        return -1;
    }
    *created_seconds = metadata.st_birthtimespec.tv_sec > 0 ? (uint64_t)metadata.st_birthtimespec.tv_sec : 0;
    *modified_seconds = metadata.st_mtimespec.tv_sec > 0 ? (uint64_t)metadata.st_mtimespec.tv_sec : 0;
    return 0;
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
    const unsigned char *buffer_end = directory->buffer + DS_BUFFER_SIZE;
    if (group > buffer_end || sizeof(uint32_t) > (size_t)(buffer_end - group)) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    const unsigned char *cursor = group;
    uint32_t group_length = 0;
    attribute_set_t returned;
    attrreference_t name_reference;
    dev_t device = 0;
    fsobj_type_t object_type = 0;
    off_t logical_size = 0;
    off_t allocated_size = 0;
    off_t private_size = 0;

    if (ds_take(&cursor, buffer_end, &group_length, sizeof(group_length)) != 0) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    if (group_length < sizeof(uint32_t) + sizeof(attribute_set_t) ||
        group_length > DS_BUFFER_SIZE ||
        group + group_length > directory->buffer + DS_BUFFER_SIZE) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    directory->cursor += group_length;
    directory->remaining -= 1;

    const unsigned char *group_end = group + group_length;
    if (ds_take(&cursor, group_end, &returned, sizeof(returned)) != 0 ||
        !(returned.commonattr & ATTR_CMN_ERROR) || !(returned.commonattr & ATTR_CMN_NAME) ||
        !(returned.commonattr & ATTR_CMN_DEVID) || !(returned.commonattr & ATTR_CMN_OBJTYPE) ||
        !(returned.commonattr & ATTR_CMN_FILEID) ||
        ds_take(&cursor, group_end, &output->error_code, sizeof(output->error_code)) != 0) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    const unsigned char *name_reference_location = cursor;
    if (ds_take(&cursor, group_end, &name_reference, sizeof(name_reference)) != 0 ||
        ds_take(&cursor, group_end, &device, sizeof(device)) != 0 ||
        ds_take(&cursor, group_end, &object_type, sizeof(object_type)) != 0 ||
        ds_take(&cursor, group_end, &output->file_id, sizeof(output->file_id)) != 0) {
        directory->last_errno = EIO;
        errno = EIO;
        return -1;
    }
    if (returned.fileattr & ATTR_FILE_LINKCOUNT) {
        if (ds_take(&cursor, group_end, &output->link_count, sizeof(output->link_count)) != 0) { goto malformed_group; }
    }
    if (returned.fileattr & ATTR_FILE_TOTALSIZE) {
        if (ds_take(&cursor, group_end, &logical_size, sizeof(logical_size)) != 0) { goto malformed_group; }
    }
    if (returned.fileattr & ATTR_FILE_ALLOCSIZE) {
        if (ds_take(&cursor, group_end, &allocated_size, sizeof(allocated_size)) != 0) { goto malformed_group; }
    }
    if (returned.forkattr & ATTR_CMNEXT_PRIVATESIZE) {
        if (ds_take(&cursor, group_end, &private_size, sizeof(private_size)) != 0) { goto malformed_group; }
    }

    if (name_reference.attr_dataoffset < 0 || (uint32_t)name_reference.attr_dataoffset > group_length ||
        name_reference.attr_length > group_length - (uint32_t)name_reference.attr_dataoffset) {
        goto malformed_group;
    }
    const unsigned char *name = name_reference_location + name_reference.attr_dataoffset;
    size_t name_length = name_reference.attr_length;
    if (name_length > 0 && name[name_length - 1] == '\0') {
        name_length -= 1;
    }
    if (name < group || name_length > (size_t)(group_end - name)) {
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

malformed_group:
    directory->last_errno = EIO;
    errno = EIO;
    return -1;
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
