/* Trusted local file/process adapters. An exec grant authorizes a program and
 * its chosen arguments; it is not an OS sandbox or transitive authority filter. */
#include <sys/wait.h>
#ifdef __APPLE__
#include <spawn.h>
extern char **environ;
#endif

static KResult k_write_text(KText path, KText value) {
  if (!k_allowed("--allow-write=", path)) k_fail("permission_denied_write", 0);
  if (!path.len || path.len > 4096 || !k_cstr(path) || value.len > K_STD_LIMIT)
    return k_err(K_TEXT("invalid write path or text exceeds 1 MiB"));
  KText name = k_clone(path);
  struct stat st;
  if (lstat(name.ptr, &st) == 0) {
    if (!S_ISREG(st.st_mode)) { k_drop(&name); return k_err(K_TEXT("write target must be a regular file")); }
  } else if (errno != ENOENT) { k_drop(&name); return k_err(K_TEXT("cannot inspect write target")); }
  KText temporary = k_concat(name, K_TEXT(".keel-write-XXXXXX"));
  int fd = mkstemp((char *)temporary.ptr);
  if (fd < 0) { k_drop(&name); k_drop(&temporary); return k_err(K_TEXT("cannot create atomic write")); }
  size_t used = 0; bool success = true;
  while (used < value.len) {
    ssize_t n = write(fd, value.ptr + used, value.len - used);
    if (n < 0 && errno == EINTR) continue;
    if (n <= 0) { success = false; break; }
    used += (size_t)n;
  }
  if (close(fd)) success = false;
  if (success && rename(temporary.ptr, name.ptr)) success = false;
  if (!success) unlink(temporary.ptr);
  k_drop(&name); k_drop(&temporary);
  return success ? k_ok((int64_t)used) : k_err(K_TEXT("atomic write failed"));
}
static int64_t k_clock_millis(void) {
  if (!k_allowed("--allow-clock=", K_TEXT("monotonic"))) k_fail("permission_denied_clock", 0);
  return k_millis();
}

typedef struct { int64_t handle; pid_t pid; } KChild;
static KChild k_children[32];
static int64_t k_next_child = 1;
static bool k_child_cleanup_registered = false;
static sigset_t k_process_block_signals(void) {
  sigset_t blocked, previous;
  sigemptyset(&blocked); sigaddset(&blocked, SIGINT); sigaddset(&blocked, SIGTERM);
  if (sigprocmask(SIG_BLOCK, &blocked, &previous)) k_fail("process_signal_failed", 0);
  return previous;
}
static void k_process_restore_signals(sigset_t previous) {
  if (sigprocmask(SIG_SETMASK, &previous, NULL)) k_fail("process_signal_failed", 0);
}
static void k_children_cleanup(void) {
  sigset_t previous = k_process_block_signals();
  for (size_t i = 0; i < 32; i++) if (k_children[i].pid > 0) kill(-k_children[i].pid, SIGKILL);
  for (size_t i = 0; i < 32; i++) if (k_children[i].pid > 0) {
    while (waitpid(k_children[i].pid, NULL, 0) < 0 && errno == EINTR) {}
    k_children[i] = (KChild){0};
  }
  k_process_restore_signals(previous);
}
static void k_children_signal(int signal_number) {
  for (size_t i = 0; i < 32; i++) if (k_children[i].pid > 0) kill(-k_children[i].pid, SIGKILL);
  _exit(128 + signal_number);
}
static int k_child_slot(void) {
  if (!k_child_cleanup_registered) {
    atexit(k_children_cleanup);
    signal(SIGINT, k_children_signal); signal(SIGTERM, k_children_signal);
    k_child_cleanup_registered = true;
  }
  if (k_next_child == INT64_MAX) return -1;
  for (int i = 0; i < 32; i++) if (!k_children[i].pid) return i;
  return -1;
}
/* argv is a JSON array because this language currently has no List<Text>.
 * Exact executable path, 64 arguments, 64 KiB combined; no shell expansion. */
static bool k_process_argv(KText program, KText arguments, KText *owned, char **argv, size_t *count) {
  if (!k_allowed("--allow-exec=", program)) k_fail("permission_denied_exec", 0);
  *count = 0;
  if (!program.len || program.len > 4096 || !k_cstr(program) || !memchr(program.ptr, '/', program.len) || arguments.len > 65536) return false;
  owned[0] = k_clone(program); argv[0] = (char *)owned[0].ptr; *count = 1;
  KJson parser = {arguments, 0, false}; k_json_ws(&parser);
  if (parser.pos >= arguments.len || arguments.ptr[parser.pos++] != '[') return false;
  k_json_ws(&parser);
  if (parser.pos < arguments.len && arguments.ptr[parser.pos] == ']') parser.pos++;
  else for (;;) {
    if (*count >= 65 || parser.pos >= arguments.len || arguments.ptr[parser.pos] != '"') return false;
    KText arg = k_json_string(&parser, true);
    owned[*count] = arg; argv[*count] = (char *)arg.ptr; (*count)++;
    if (parser.bad || !k_cstr(arg) || !k_utf8(arg)) return false;
    k_json_ws(&parser);
    if (parser.pos >= arguments.len) return false;
    char next = arguments.ptr[parser.pos++];
    if (next == ']') break;
    if (next != ',') return false;
    k_json_ws(&parser);
  }
  k_json_ws(&parser);
  argv[*count] = NULL;
  return parser.pos == arguments.len && !parser.bad;
}
static void k_process_args_drop(KText *owned, size_t count) {
  for (size_t i = 0; i < count; i++) k_drop(&owned[i]);
}
static pid_t k_process_start(char **argv, int output, int slot) {
  sigset_t previous = k_process_block_signals();
#ifdef __APPLE__
  /* CLOEXEC_DEFAULT closes every descriptor except those explicitly inherited
   * through file actions, without scanning the often enormous OPEN_MAX range. */
  posix_spawn_file_actions_t actions;
  posix_spawnattr_t attributes;
  int error = posix_spawn_file_actions_init(&actions);
  if (error) { k_process_restore_signals(previous); errno = error; return -1; }
  error = posix_spawnattr_init(&attributes);
  if (error) {
    posix_spawn_file_actions_destroy(&actions);
    k_process_restore_signals(previous); errno = error; return -1;
  }
  sigset_t defaults;
  sigemptyset(&defaults); sigaddset(&defaults, SIGINT);
  sigaddset(&defaults, SIGTERM); sigaddset(&defaults, SIGPIPE);
  if (!error) error = posix_spawnattr_setsigmask(&attributes, &previous);
  if (!error) error = posix_spawnattr_setsigdefault(&attributes, &defaults);
  if (!error) error = posix_spawnattr_setpgroup(&attributes, 0);
  if (!error) error = posix_spawnattr_setflags(&attributes,
    POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETSIGMASK |
    POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETPGROUP);
  if (!error) error = posix_spawn_file_actions_addopen(&actions, STDIN_FILENO, "/dev/null", O_RDWR, 0);
  if (!error) error = posix_spawn_file_actions_adddup2(&actions, output >= 0 ? output : STDIN_FILENO, STDOUT_FILENO);
  if (!error) error = posix_spawn_file_actions_adddup2(&actions, output >= 0 ? output : STDIN_FILENO, STDERR_FILENO);
  if (!error && output > STDERR_FILENO) error = posix_spawn_file_actions_addclose(&actions, output);
  pid_t pid = -1;
  if (!error) error = posix_spawn(&pid, argv[0], &actions, &attributes, argv, environ);
  posix_spawnattr_destroy(&attributes);
  posix_spawn_file_actions_destroy(&actions);
  if (error) pid = -1;
#else
  pid_t pid = fork();
  if (pid == 0) {
    setpgid(0, 0);
    signal(SIGINT, SIG_DFL); signal(SIGTERM, SIG_DFL); signal(SIGPIPE, SIG_DFL);
    sigprocmask(SIG_SETMASK, &previous, NULL);
    int null = open("/dev/null", O_RDWR);
    if (null < 0 || dup2(null, STDIN_FILENO) < 0 || dup2(output >= 0 ? output : null, STDOUT_FILENO) < 0 || dup2(output >= 0 ? output : null, STDERR_FILENO) < 0) _exit(126);
    /* No server listener, client socket, file descriptor, or pipe leaks into the child. */
    long limit = sysconf(_SC_OPEN_MAX);
    if (limit < 0) limit = 1024;
    for (long fd = 3; fd < limit; fd++) close((int)fd);
    execv(argv[0], argv);
    _exit(127);
  }
#endif
  if (pid > 0) {
    setpgid(pid, pid);
    k_children[slot] = (KChild){k_next_child++, pid};
  }
  k_process_restore_signals(previous);
  return pid;
}
static KResult k_process_spawn(KText program, KText arguments) {
  KText owned[65] = {{0}}; char *argv[66]; size_t count;
  if (!k_process_argv(program, arguments, owned, argv, &count)) {
    k_process_args_drop(owned, count); return k_err(K_TEXT("invalid executable path or JSON argument array"));
  }
  int slot = k_child_slot();
  if (slot < 0) { k_process_args_drop(owned, count); return k_err(K_TEXT("process handle limit reached; poll completed children")); }
  pid_t pid = k_process_start(argv, -1, slot);
  k_process_args_drop(owned, count);
  if (pid < 0) return k_err(K_TEXT("process start failed"));
  return k_ok(k_children[slot].handle);
}
static int k_process_find(int64_t handle) {
  for (int i = 0; i < 32; i++) if (k_children[i].pid > 0 && k_children[i].handle == handle) return i;
  return -1;
}
static int64_t k_process_status(int status) {
  return WIFEXITED(status) ? WEXITSTATUS(status) : WIFSIGNALED(status) ? 128 + WTERMSIG(status) : 255;
}
static pid_t k_process_collect(int slot, int *status) {
  sigset_t previous = k_process_block_signals();
  pid_t pid = k_children[slot].pid;
  siginfo_t info = {0};
  if (waitid(P_PID, (id_t)pid, &info, WEXITED | WNOHANG | WNOWAIT) < 0) {
    int saved = errno;
    k_process_restore_signals(previous); errno = saved; return -1;
  }
  if (!info.si_pid) { k_process_restore_signals(previous); return 0; }
  // Keep the leader unreaped until descendants are killed, so its process-group
  // identifier cannot be recycled before cleanup.
  kill(-pid, SIGKILL);
  pid_t result;
  do { result = waitpid(pid, status, 0); } while (result < 0 && errno == EINTR);
  k_children[slot] = (KChild){0};
  k_process_restore_signals(previous);
  return result;
}
static KResult k_process_poll(int64_t handle) {
  int slot = k_process_find(handle);
  if (slot < 0) return k_err(K_TEXT("unknown process handle"));
  int status; pid_t result = k_process_collect(slot, &status);
  if (!result || (result < 0 && errno == EINTR)) return k_ok(-1);
  if (result < 0) return k_err(K_TEXT("process wait failed"));
  k_children[slot] = (KChild){0};
  return k_ok(k_process_status(status));
}
static KResult k_process_terminate(int64_t handle) {
  int slot = k_process_find(handle);
  if (slot < 0) return k_err(K_TEXT("unknown process handle"));
  sigset_t previous = k_process_block_signals();
  kill(-k_children[slot].pid, SIGKILL);
  int status; pid_t result;
  do { result = waitpid(k_children[slot].pid, &status, 0); } while (result < 0 && errno == EINTR);
  k_children[slot] = (KChild){0};
  k_process_restore_signals(previous);
  return result < 0 ? k_err(K_TEXT("process wait failed")) : k_ok(k_process_status(status));
}
static KTextResult k_process_run_timeout(KText program, KText arguments, int64_t timeout_ms) {
  if (!k_allowed("--allow-exec=", program)) k_fail("permission_denied_exec", 0);
  if (timeout_ms < 1 || timeout_ms > 30000) return k_text_error("process timeout must be 1..30000 ms");
  KText owned[65] = {{0}}; char *argv[66]; size_t count;
  if (!k_process_argv(program, arguments, owned, argv, &count)) {
    k_process_args_drop(owned, count); return k_text_error("invalid executable path or JSON argument array");
  }
  int slot = k_child_slot(), pipes[2];
  if (slot < 0 || pipe(pipes)) { k_process_args_drop(owned, count); return k_text_error("process resource limit"); }
  pid_t pid = k_process_start(argv, pipes[1], slot);
  k_process_args_drop(owned, count); close(pipes[1]);
  if (pid < 0) { close(pipes[0]); return k_text_error("process start failed"); }
  int flags = fcntl(pipes[0], F_GETFL, 0);
  if (flags < 0 || fcntl(pipes[0], F_SETFL, flags | O_NONBLOCK) < 0) {
    k_process_terminate(k_children[slot].handle); close(pipes[0]); return k_text_error("process pipe failed");
  }
  KText output = k_alloc(K_STD_LIMIT + 1); size_t used = 0;
  int64_t deadline = k_millis() + timeout_ms;
  bool eof = false, done = false, failed = false; int status = 0;
  while (!eof || !done) {
    if (k_millis() >= deadline) { failed = true; break; }
    if (!eof) {
      ssize_t n = read(pipes[0], (char *)output.ptr + used, K_STD_LIMIT + 1 - used);
      if (n > 0) { used += (size_t)n; if (used > K_STD_LIMIT) { failed = true; break; } }
      else if (!n) eof = true;
      else if (errno != EINTR && errno != EAGAIN && errno != EWOULDBLOCK) { failed = true; break; }
    }
    if (!done) {
      pid_t result = k_process_collect(slot, &status);
      if (result == pid) done = true;
      else if (result < 0 && errno != EINTR) { failed = true; break; }
    }
    if (!eof || !done) {
      struct pollfd event = {pipes[0], POLLIN, 0};
      // An EOF pipe is permanently readable (POLLHUP); don't spin on it while
      // waiting for a child that closed output early.
      poll(eof ? NULL : &event, eof ? 0 : 1, 5);
    }
  }
  if (!done) {
    k_process_terminate(k_children[slot].handle);
  }
  k_children[slot] = (KChild){0}; close(pipes[0]);
  output.len = used; ((char *)output.ptr)[used] = 0;
  if (failed || !k_utf8(output)) { k_drop(&output); return k_text_error("process timeout, output limit, or invalid UTF-8"); }
  if (k_process_status(status) != 0) {
    if (!used) { k_drop(&output); return k_text_error("process exited unsuccessfully"); }
    return k_text_err(output);
  }
  return k_text_ok(output);
}
static KTextResult k_process_run(KText program, KText arguments) {
  return k_process_run_timeout(program, arguments, 30000);
}
