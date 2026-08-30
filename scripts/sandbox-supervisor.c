#define _GNU_SOURCE

#include <sys/prctl.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 2 || prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) != 0) {
        return 126;
    }
    execv(argv[1], &argv[1]);
    return 126;
}
