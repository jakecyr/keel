/* Trusted, bounded standard adapters. Text inputs and outputs are UTF-8. */
#include <arpa/inet.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <sys/stat.h>
#include <time.h>
#define K_STD_LIMIT (1024u * 1024u)
#define K_TEXT(s) ((KText){s, sizeof(s) - 1, false})
static bool k_utf8(KText s) {
  for (size_t i = 0; i < s.len;) {
    unsigned c = (unsigned char)s.ptr[i++];
    if (c < 128)
      continue;
    unsigned n, min, v;
    if (c >= 0xc2 && c <= 0xdf) {
      n = 1;
      min = 128;
      v = c & 31;
    } else if (c >= 0xe0 && c <= 0xef) {
      n = 2;
      min = 2048;
      v = c & 15;
    } else if (c >= 0xf0 && c <= 0xf4) {
      n = 3;
      min = 65536;
      v = c & 7;
    } else
      return false;
    if (n > s.len - i)
      return false;
    while (n--) {
      c = (unsigned char)s.ptr[i++];
      if ((c & 0xc0) != 0x80)
        return false;
      v = (v << 6) | (c & 63);
    }
    if (v < min || v > 0x10ffff || (v >= 0xd800 && v <= 0xdfff))
      return false;
  }
  return true;
}
static bool k_cstr(KText t) { return !memchr(t.ptr, 0, t.len); }
static KText k_slice(KText t, size_t a, size_t b) {
  return (KText){t.ptr + a, b - a, false};
}
/* JSON retains source spelling (including decimal numbers) and uses RFC 6901
 * pointers. */
typedef struct {
  KText source;
  size_t pos;
  bool bad;
} KJson;
static void k_json_ws(KJson *j) {
  while (j->pos < j->source.len && strchr(" \t\r\n", j->source.ptr[j->pos]) &&
         j->source.ptr[j->pos])
    j->pos++;
}
static int k_hex(char c) {
  return c >= '0' && c <= '9'   ? c - '0'
         : c >= 'a' && c <= 'f' ? c - 'a' + 10
         : c >= 'A' && c <= 'F' ? c - 'A' + 10
                                : -1;
}
static unsigned k_json_hex(KJson *j) {
  unsigned v = 0;
  for (int n = 0; n < 4; n++) {
    if (j->pos >= j->source.len) {
      j->bad = true;
      return 0;
    }
    int d = k_hex(j->source.ptr[j->pos++]);
    if (d < 0)
      j->bad = true;
    v = (v << 4) | (unsigned)(d < 0 ? 0 : d);
  }
  return v;
}
static KText k_json_string(KJson *j, bool decode) {
  KText out = decode ? k_alloc(j->source.len - j->pos) : (KText){0};
  size_t used = 0;
  if (j->pos >= j->source.len || j->source.ptr[j->pos++] != '"') {
    j->bad = true;
    return out;
  }
  while (j->pos < j->source.len) {
    unsigned c = (unsigned char)j->source.ptr[j->pos++];
    if (c == '"') {
      if (decode) {
        ((char *)out.ptr)[used] = 0;
        out.len = used;
      }
      return out;
    }
    if (c < 32) {
      j->bad = true;
      break;
    }
    if (c == '\\') {
      if (j->pos >= j->source.len) {
        j->bad = true;
        break;
      }
      char e = j->source.ptr[j->pos++];
      if (e == 'u') {
        c = k_json_hex(j);
        if (c >= 0xd800 && c <= 0xdbff) {
          if (j->pos + 2 > j->source.len || j->source.ptr[j->pos] != '\\' ||
              j->source.ptr[j->pos + 1] != 'u') {
            j->bad = true;
            break;
          }
          j->pos += 2;
          unsigned low = k_json_hex(j);
          if (low < 0xdc00 || low > 0xdfff) {
            j->bad = true;
            break;
          }
          c = 0x10000 + ((c - 0xd800) << 10) + (low - 0xdc00);
        } else if (c >= 0xdc00 && c <= 0xdfff) {
          j->bad = true;
          break;
        }
        if (decode) {
          char *p = (char *)out.ptr;
          if (c < 128)
            p[used++] = (char)c;
          else if (c < 2048) {
            p[used++] = (char)(0xc0 | (c >> 6));
            p[used++] = (char)(0x80 | (c & 63));
          } else if (c < 65536) {
            p[used++] = (char)(0xe0 | (c >> 12));
            p[used++] = (char)(0x80 | ((c >> 6) & 63));
            p[used++] = (char)(0x80 | (c & 63));
          } else {
            p[used++] = (char)(0xf0 | (c >> 18));
            p[used++] = (char)(0x80 | ((c >> 12) & 63));
            p[used++] = (char)(0x80 | ((c >> 6) & 63));
            p[used++] = (char)(0x80 | (c & 63));
          }
        }
        continue;
      }
      switch (e) {
      case '"':
        c = '"';
        break;
      case '\\':
        c = '\\';
        break;
      case '/':
        c = '/';
        break;
      case 'b':
        c = 8;
        break;
      case 'f':
        c = 12;
        break;
      case 'n':
        c = 10;
        break;
      case 'r':
        c = 13;
        break;
      case 't':
        c = 9;
        break;
      default:
        j->bad = true;
      }
    }
    if (j->bad)
      break;
    if (decode)
      ((char *)out.ptr)[used++] = (char)c;
  }
  j->bad = true;
  return out;
}
static void k_json_value(KJson *j, unsigned depth) {
  k_json_ws(j);
  if (j->bad || j->pos >= j->source.len || depth > 64) {
    j->bad = true;
    return;
  }
  char c = j->source.ptr[j->pos];
  if (c == '"') {
    k_json_string(j, false);
    return;
  }
  if (c == '{' || c == '[') {
    char end = c == '{' ? '}' : ']';
    j->pos++;
    k_json_ws(j);
    if (j->pos < j->source.len && j->source.ptr[j->pos] == end) {
      j->pos++;
      return;
    }
    for (;;) {
      if (c == '{') {
        k_json_ws(j);
        k_json_string(j, false);
        k_json_ws(j);
        if (j->pos >= j->source.len || j->source.ptr[j->pos++] != ':')
          j->bad = true;
      }
      k_json_value(j, depth + 1);
      k_json_ws(j);
      if (j->bad || j->pos >= j->source.len) {
        j->bad = true;
        return;
      }
      char next = j->source.ptr[j->pos++];
      if (next == end)
        return;
      if (next != ',') {
        j->bad = true;
        return;
      }
    }
  }
  const char *literal = c == 't'   ? "true"
                        : c == 'f' ? "false"
                        : c == 'n' ? "null"
                                   : NULL;
  if (literal) {
    size_t n = strlen(literal);
    if (n > j->source.len - j->pos ||
        memcmp(j->source.ptr + j->pos, literal, n))
      j->bad = true;
    else
      j->pos += n;
    return;
  }
  size_t start = j->pos;
  if (c == '-')
    j->pos++;
  if (j->pos < j->source.len && j->source.ptr[j->pos] == '0')
    j->pos++;
  else {
    size_t d = j->pos;
    while (j->pos < j->source.len && j->source.ptr[j->pos] >= '0' &&
           j->source.ptr[j->pos] <= '9')
      j->pos++;
    if (j->pos == d)
      j->bad = true;
  }
  if (j->pos < j->source.len && j->source.ptr[j->pos] == '.') {
    j->pos++;
    size_t d = j->pos;
    while (j->pos < j->source.len && j->source.ptr[j->pos] >= '0' &&
           j->source.ptr[j->pos] <= '9')
      j->pos++;
    if (j->pos == d)
      j->bad = true;
  }
  if (j->pos < j->source.len &&
      (j->source.ptr[j->pos] == 'e' || j->source.ptr[j->pos] == 'E')) {
    j->pos++;
    if (j->pos < j->source.len &&
        (j->source.ptr[j->pos] == '+' || j->source.ptr[j->pos] == '-'))
      j->pos++;
    size_t d = j->pos;
    while (j->pos < j->source.len && j->source.ptr[j->pos] >= '0' &&
           j->source.ptr[j->pos] <= '9')
      j->pos++;
    if (j->pos == d)
      j->bad = true;
  }
  if (j->pos == start)
    j->bad = true;
}
static bool k_json_valid(KText s) {
  if (s.len > K_STD_LIMIT || !k_utf8(s))
    return false;
  KJson j = {s, 0, false};
  k_json_value(&j, 0);
  k_json_ws(&j);
  return !j.bad && j.pos == s.len;
}
static KTextResult k_json_parse(KText s) {
  return k_json_valid(s) ? k_text_ok(k_clone(s)) : k_text_error("invalid JSON");
}
static KTextResult k_json_get(KText s, KText pointer) {
  if (!k_json_valid(s))
    return k_text_error("invalid JSON");
  if (pointer.len > K_STD_LIMIT || (pointer.len && pointer.ptr[0] != '/'))
    return k_text_error("invalid JSON pointer");
  KJson root = {s, 0, false};
  k_json_ws(&root);
  size_t start = root.pos;
  k_json_value(&root, 0);
  KText current = k_slice(s, start, root.pos);
  for (size_t p = 0; p < pointer.len;) {
    size_t end = ++p;
    while (end < pointer.len && pointer.ptr[end] != '/')
      end++;
    KText key = k_alloc(end - p);
    size_t n = 0;
    while (p < end) {
      char c = pointer.ptr[p++];
      if (c == '~') {
        if (p == end || (pointer.ptr[p] != '0' && pointer.ptr[p] != '1')) {
          k_drop(&key);
          return k_text_error("invalid JSON pointer");
        }
        c = pointer.ptr[p++] == '0' ? '~' : '/';
      }
      ((char *)key.ptr)[n++] = c;
    }
    key.len = n;
    ((char *)key.ptr)[n] = 0;
    KJson j = {current, 1, false};
    bool object = current.ptr[0] == '{', array = current.ptr[0] == '[';
    KText found = {0};
    size_t index = 0, wanted = 0;
    bool index_ok = n > 0 && !(n > 1 && key.ptr[0] == '0');
    if (array)
      for (size_t i = 0; i < n; i++) {
        if (key.ptr[i] < '0' || key.ptr[i] > '9' || wanted > K_STD_LIMIT) {
          index_ok = false;
          break;
        }
        wanted = wanted * 10 + (unsigned)(key.ptr[i] - '0');
      }
    if (object || array)
      for (;;) {
        k_json_ws(&j);
        if (j.pos >= current.len || current.ptr[j.pos] == '}' ||
            current.ptr[j.pos] == ']')
          break;
        bool match = false;
        if (object) {
          KText name = k_json_string(&j, true);
          match = k_equal(name, key);
          k_drop(&name);
          k_json_ws(&j);
          j.pos++;
        } else
          match = index_ok && index == wanted;
        k_json_ws(&j);
        size_t a = j.pos;
        k_json_value(&j, 0);
        if (match)
          found = k_slice(current, a, j.pos);
        index++;
        k_json_ws(&j);
        if (j.pos < current.len && current.ptr[j.pos] == ',')
          j.pos++;
        else
          break;
      }
    k_drop(&key);
    if (!found.ptr)
      return k_text_error("JSON path not found");
    current = found;
  }
  return k_text_ok(k_clone(current));
}
static KTextResult k_json_text(KText s, KText path) {
  KTextResult r = k_json_get(s, path);
  if (!r.ok)
    return r;
  if (!r.value.len || r.value.ptr[0] != '"') {
    k_drop(&r);
    return k_text_error("expected JSON string");
  }
  KJson j = {r.value, 0, false};
  KText out = k_json_string(&j, true);
  k_drop(&r);
  return k_text_ok(out);
}
static KResult k_json_int(KText s, KText path) {
  KTextResult r = k_json_get(s, path);
  if (!r.ok) {
    KText e = k_move(&r.error);
    k_drop(&r);
    return k_err(e);
  }
  KResult out = k_parse_int(r.value);
  k_drop(&r);
  return out;
}
static KText k_json_quote(KText s) {
  if (s.len > K_STD_LIMIT)
    k_fail("resource_limit", 0);
  KText out = k_alloc(s.len * 6 + 2);
  size_t n = 0;
  char *p = (char *)out.ptr;
  p[n++] = '"';
  const char *hex = "0123456789abcdef";
  for (size_t i = 0; i < s.len; i++) {
    unsigned char c = (unsigned char)s.ptr[i];
    if (c == '"' || c == '\\') {
      p[n++] = '\\';
      p[n++] = (char)c;
    } else if (c < 32) {
      p[n++] = '\\';
      p[n++] = 'u';
      p[n++] = '0';
      p[n++] = '0';
      p[n++] = hex[c >> 4];
      p[n++] = hex[c & 15];
    } else
      p[n++] = (char)c;
  }
  p[n++] = '"';
  p[n] = 0;
  out.len = n;
  return out;
}
static KTextResult k_json_response(int64_t status, KText body) {
  if (status < 100 || status > 599)
    return k_text_error("invalid HTTP status");
  if (!k_json_valid(body))
    return k_text_error("invalid JSON");
  char header[256];
  int n =
      snprintf(header, sizeof(header),
               "HTTP/1.1 %" PRId64
               " Response\r\nContent-Type: application/json\r\nContent-Length: "
               "%zu\r\nConnection: close\r\n\r\n",
               status, body.len);
  return k_text_ok(k_concat((KText){header, (size_t)n, false}, body));
}
/* RFC 4180-style cells; strict quotes, CRLF/LF records, embedded newlines. */
static KTextResult k_csv_get(KText s, int64_t row, int64_t column) {
  if (s.len > K_STD_LIMIT || !k_utf8(s))
    return k_text_error("invalid CSV");
  if (row < 0 || column < 0)
    return k_text_error("CSV cell not found");
  KText field = k_alloc(s.len), selected = {0};
  size_t p = 0, r = 0, c = 0;
  while (p < s.len) {
    size_t n = 0;
    bool quoted = s.ptr[p] == '"', closed = !quoted;
    if (quoted)
      p++;
    while (p < s.len) {
      char ch = s.ptr[p];
      if (quoted) {
        p++;
        if (ch == '"') {
          if (p < s.len && s.ptr[p] == '"') {
            p++;
            ((char *)field.ptr)[n++] = '"';
          } else {
            closed = true;
            break;
          }
        } else
          ((char *)field.ptr)[n++] = ch;
      } else {
        if (ch == ',' || ch == '\r' || ch == '\n')
          break;
        if (ch == '"')
          goto invalid_csv;
        ((char *)field.ptr)[n++] = ch;
        p++;
      }
    }
    if (!closed)
      goto invalid_csv;
    if (p < s.len && s.ptr[p] != ',' && s.ptr[p] != '\r' && s.ptr[p] != '\n')
      goto invalid_csv;
    if (r == (uint64_t)row && c == (uint64_t)column) {
      k_drop(&selected);
      selected = k_clone((KText){field.ptr, n, false});
    }
    if (p == s.len)
      break;
    if (s.ptr[p++] == ',') {
      c++;
      if (p == s.len && r == (uint64_t)row && c == (uint64_t)column)
        selected = k_alloc(0);
    } else {
      if (s.ptr[p - 1] == '\r') {
        if (p == s.len || s.ptr[p] != '\n')
          goto invalid_csv;
        p++;
      }
      r++;
      c = 0;
    }
  }
  k_drop(&field);
  return selected.ptr ? k_text_ok(selected)
                      : k_text_error("CSV cell not found");
invalid_csv:
  k_drop(&field);
  k_drop(&selected);
  return k_text_error("invalid CSV");
}
/* Buffered SSE: only dispatched events with at least one data field count. */
static KTextResult k_sse_data(KText s, int64_t wanted) {
  if (s.len > K_STD_LIMIT || !k_utf8(s))
    return k_text_error("invalid SSE");
  if (wanted < 0)
    return k_text_error("SSE event not found");
  size_t p = 0, n = 0, index = 0;
  bool data = false;
  KText out = k_alloc(s.len);
  if (s.len >= 3 && !memcmp(s.ptr, "\xef\xbb\xbf", 3))
    p = 3;
  while (p < s.len) {
    size_t start = p;
    while (p < s.len && s.ptr[p] != '\r' && s.ptr[p] != '\n')
      p++;
    size_t end = p;
    if (p == s.len)
      break;
    char e = s.ptr[p++];
    if (e == '\r' && p < s.len && s.ptr[p] == '\n')
      p++;
    if (end == start) {
      if (data && index++ == (uint64_t)wanted) {
        out.len = n - 1;
        ((char *)out.ptr)[out.len] = 0;
        return k_text_ok(out);
      }
      n = 0;
      data = false;
    } else if (end - start >= 4 && !memcmp(s.ptr + start, "data", 4) &&
               (end - start == 4 || s.ptr[start + 4] == ':')) {
      size_t a = start + 4;
      if (a < end)
        a++;
      if (a < end && s.ptr[a] == ' ')
        a++;
      memcpy((char *)out.ptr + n, s.ptr + a, end - a);
      n += end - a;
      ((char *)out.ptr)[n++] = '\n';
      data = true;
    }
  }
  k_drop(&out);
  return k_text_error("SSE event not found");
}
/* Permissions are exact strings, held outside source by the launcher. */
static const char *k_std_permissions[256];
static size_t k_std_permission_count = 0;
static bool k_std_permission(const char *arg) {
  if (strncmp(arg, "--allow-connect=", 16) &&
      strncmp(arg, "--allow-read=", 13) && strncmp(arg, "--allow-env=", 12) &&
      strncmp(arg, "--allow-write=", 14) && strncmp(arg, "--allow-exec=", 13) &&
      strncmp(arg, "--allow-clock=", 14))
    return false;
  if (k_std_permission_count == 256)
    k_fail("permission_limit", 0);
  k_std_permissions[k_std_permission_count++] = arg;
  return true;
}
static bool k_allowed(const char *prefix, KText value) {
  size_t n = strlen(prefix);
  for (size_t i = 0; i < k_std_permission_count; i++) {
    const char *p = k_std_permissions[i];
    if (!strncmp(p, prefix, n) && strlen(p + n) == value.len &&
        !memcmp(p + n, value.ptr, value.len))
      return true;
  }
  return false;
}
static KTextResult k_env_get(KText name) {
  if (!k_allowed("--allow-env=", name))
    k_fail("permission_denied_env", 0);
  if (!k_cstr(name))
    return k_text_error("invalid environment name");
  KText key = k_clone(name);
  const char *value = getenv(key.ptr);
  k_drop(&key);
  if (!value)
    return k_text_error("environment variable not set");
  size_t n = strnlen(value, K_STD_LIMIT + 1);
  KText t = {value, n, false};
  if (n > K_STD_LIMIT || !k_utf8(t))
    return k_text_error("invalid environment text");
  return k_text_ok(k_clone(t));
}
static KTextResult k_read_text(KText path) {
  if (!k_allowed("--allow-read=", path))
    k_fail("permission_denied_fs", 0);
  if (!k_cstr(path))
    return k_text_error("invalid file path");
  KText name = k_clone(path);
  int fd = open(name.ptr, O_RDONLY | O_NONBLOCK | O_NOFOLLOW);
  k_drop(&name);
  if (fd < 0)
    return k_text_error("file read failed");
  struct stat st;
  if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size < 0 ||
      st.st_size > K_STD_LIMIT) {
    close(fd);
    return k_text_error("file must be regular and at most 1 MiB");
  }
  KText data = k_alloc(K_STD_LIMIT + 1);
  size_t used = 0;
  while (used <= K_STD_LIMIT) {
    ssize_t n = read(fd, (char *)data.ptr + used, K_STD_LIMIT + 1 - used);
    if (n < 0 && errno == EINTR)
      continue;
    if (n < 0) {
      close(fd);
      k_drop(&data);
      return k_text_error("file read failed");
    }
    if (!n)
      break;
    used += (size_t)n;
  }
  close(fd);
  data.len = used;
  ((char *)data.ptr)[used] = 0;
  if (used > K_STD_LIMIT || !k_utf8(data)) {
    k_drop(&data);
    return k_text_error("file must be UTF-8 and at most 1 MiB");
  }
  return k_text_ok(data);
}
/* A scoped fork/join: copied integers in, ordered integers out, at most four
 * workers. */
typedef struct {
  KList input;
  KList output;
  size_t first, stride;
  int64_t (*fn)(int64_t);
} KJob;
static _Thread_local bool k_in_parallel = false;
static void *k_parallel_worker(void *arg) {
  KJob *j = arg;
  k_in_parallel = true;
  for (size_t i = j->first; i < j->input.len; i += j->stride)
    j->output.ptr[i] = j->fn(j->input.ptr[i]);
  k_in_parallel = false;
  return NULL;
}
static KList k_parallel_map(KList input, int64_t (*fn)(int64_t)) {
  if (input.len > 100000)
    k_fail("resource_limit", 0);
  KList out = k_list_clone(input);
  if (k_in_parallel || input.len < 2) {
    for (size_t i = 0; i < input.len; i++)
      out.ptr[i] = fn(input.ptr[i]);
    return out;
  }
  size_t count = input.len < 4 ? input.len : 4;
  pthread_t threads[4];
  KJob jobs[4];
  size_t started = 0;
  for (size_t i = 0; i < count; i++) {
    jobs[i] = (KJob){input, out, i, count, fn};
    if (pthread_create(&threads[i], NULL, k_parallel_worker, &jobs[i])) {
      for (size_t n = 0; n < started; n++)
        pthread_join(threads[n], NULL);
      k_drop(&out);
      k_fail("thread_start_failed", 0);
    }
    started++;
  }
  for (size_t i = 0; i < count; i++)
    if (pthread_join(threads[i], NULL))
      k_fail("thread_join_failed", 0);
  return out;
}
static int64_t k_millis(void) {
  struct timespec t;
  if (clock_gettime(CLOCK_MONOTONIC, &t))
    k_fail("clock_failed", 0);
  return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static bool k_wait_fd(int fd, short events, int64_t deadline) {
  for (;;) {
    int64_t left = deadline - k_millis();
    if (left <= 0)
      return false;
    struct pollfd p = {fd, events, 0};
    int n = poll(&p, 1, (int)left);
    if (n < 0 && errno == EINTR)
      continue;
    return n > 0 && !(p.revents & POLLNVAL);
  }
}
/* One bounded request/reply. TCP reads to EOF; UDP receives one datagram.
 * Numeric IPv4 only. */
static KTextResult k_socket_exchange(KText host, int64_t port, KText request,
                                     bool udp) {
  if (host.len > 15 || !k_cstr(host) || port < 1 || port > 65535 ||
      request.len > (udp ? 65507 : K_STD_LIMIT))
    return k_text_error("invalid socket request");
  char address[16];
  memcpy(address, host.ptr, host.len);
  address[host.len] = 0;
  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_port = htons((uint16_t)port);
  if (inet_pton(AF_INET, address, &addr.sin_addr) != 1)
    return k_text_error("expected numeric IPv4 address");
  char permission[64];
  int n = snprintf(permission, sizeof(permission), "%s://%s:%" PRId64,
                   udp ? "udp" : "tcp", address, port);
  if (!k_allowed("--allow-connect=", (KText){permission, (size_t)n, false}))
    k_fail("permission_denied_connect", 0);
  int fd = socket(AF_INET, udp ? SOCK_DGRAM : SOCK_STREAM, 0);
  if (fd < 0)
    return k_text_error("socket failed");
#ifdef SO_NOSIGPIPE
  int one = 1;
  setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof(one));
#endif
  if (fcntl(fd, F_SETFL, O_NONBLOCK) < 0) {
    close(fd);
    return k_text_error("socket failed");
  }
  int64_t deadline = k_millis() + 10000;
  if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
    if (errno != EINPROGRESS || !k_wait_fd(fd, POLLOUT, deadline))
      goto socket_error;
    int error = 0;
    socklen_t len = sizeof(error);
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &len) || error)
      goto socket_error;
  }
  size_t sent = 0;
  do {
    if (!k_wait_fd(fd, POLLOUT, deadline))
      goto socket_error;
#ifdef MSG_NOSIGNAL
    int flags = MSG_NOSIGNAL;
#else
    int flags = 0;
#endif
    ssize_t count = send(fd, request.ptr + sent, request.len - sent, flags);
    if (count < 0 && (errno == EAGAIN || errno == EINTR))
      continue;
    if (count < 0 || (!udp && count == 0 && sent < request.len))
      goto socket_error;
    sent += (size_t)count;
    if (udp) {
      if (sent != request.len)
        goto socket_error;
      break;
    }
  } while (sent < request.len);
  if (!udp)
    shutdown(fd, SHUT_WR);
  KText out = k_alloc(K_STD_LIMIT + 1);
  size_t used = 0;
  for (;;) {
    if (!k_wait_fd(fd, POLLIN, deadline)) {
      k_drop(&out);
      goto socket_error;
    }
    ssize_t count = recv(fd, (char *)out.ptr + used, K_STD_LIMIT + 1 - used, 0);
    if (count < 0 && (errno == EAGAIN || errno == EINTR))
      continue;
    if (count < 0) {
      k_drop(&out);
      goto socket_error;
    }
    used += (size_t)count;
    if (used > K_STD_LIMIT) {
      k_drop(&out);
      close(fd);
      return k_text_error("response too large");
    }
    if (udp || !count)
      break;
  }
  close(fd);
  out.len = used;
  ((char *)out.ptr)[used] = 0;
  if (!k_utf8(out)) {
    k_drop(&out);
    return k_text_error("response is not UTF-8");
  }
  return k_text_ok(out);
socket_error:
  close(fd);
  return k_text_error("socket exchange failed or timed out");
}
static KTextResult k_tcp_exchange(KText host, int64_t port, KText data) {
  return k_socket_exchange(host, port, data, false);
}
static KTextResult k_udp_exchange(KText host, int64_t port, KText data) {
  return k_socket_exchange(host, port, data, true);
}
#ifdef KEEL_CURL
#include <curl/curl.h>
typedef struct {
  KText text;
  size_t used;
} KCurlBuffer;
static size_t k_curl_write(char *p, size_t size, size_t count, void *ctx) {
  KCurlBuffer *b = ctx;
  if (count && size > SIZE_MAX / count)
    return 0;
  size_t n = size * count;
  if (n > K_STD_LIMIT - b->used)
    return 0;
  memcpy((char *)b->text.ptr + b->used, p, n);
  b->used += n;
  return n;
}
/* Origin policy includes scheme + hostname + optional explicit port, never a
 * prefix match. */
static bool k_url_allowed(KText url, bool websocket) {
  if (url.len > K_STD_LIMIT || !k_cstr(url))
    return false;
  size_t start = 0;
  if (!websocket && url.len >= 7 && !memcmp(url.ptr, "http://", 7))
    start = 7;
  if (!websocket && url.len >= 8 && !memcmp(url.ptr, "https://", 8))
    start = 8;
  if (websocket && url.len >= 5 && !memcmp(url.ptr, "ws://", 5))
    start = 5;
  if (websocket && url.len >= 6 && !memcmp(url.ptr, "wss://", 6))
    start = 6;
  if (!start)
    return false;
  size_t end = start;
  while (end < url.len && url.ptr[end] != '/' && url.ptr[end] != '?' &&
         url.ptr[end] != '#')
    end++;
  if (end == start)
    return false;
  for (size_t i = 0; i < url.len; i++) {
    unsigned char c = (unsigned char)url.ptr[i];
    if (c <= 32 || c == 127 || c == '\\')
      return false;
  }
  for (size_t i = start; i < end; i++)
    if (url.ptr[i] == '@' || url.ptr[i] == '%')
      return false;
  if (!k_allowed("--allow-connect=", k_slice(url, 0, end)))
    k_fail("permission_denied_connect", 0);
  return true;
}
static CURL *k_curl(KText url) {
  CURL *curl = curl_easy_init();
  if (!curl)
    return NULL;
  curl_easy_setopt(curl, CURLOPT_URL, url.ptr);
  curl_easy_setopt(
      curl, CURLOPT_PROXY,
      ""); /* Do not inherit ambient proxy credentials/destinations. */
  curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 0L);
  curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT_MS, 5000L);
  curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, 10000L);
  curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
  curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 1L);
  curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 2L);
  return curl;
}
static KTextResult k_http_request(KText url, KText body, KText token,
                                  bool post, int64_t timeout_ms) {
  if (timeout_ms < 1 || timeout_ms > 120000)
    return k_text_error("HTTP timeout must be 1..120000 milliseconds");
  if (!k_url_allowed(url, false))
    return k_text_error("invalid HTTP URL");
  if (post && !k_json_valid(body))
    return k_text_error("invalid JSON");
  if (token.len > 8192 || !k_cstr(token) ||
      memchr(token.ptr, '\r', token.len) || memchr(token.ptr, '\n', token.len))
    return k_text_error("invalid bearer token");
  KText address = k_clone(url);
  CURL *curl = k_curl(address);
  if (!curl) {
    k_drop(&address);
    return k_text_error("HTTP initialization failed");
  }
  curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, (long)timeout_ms);
  KCurlBuffer buffer = {k_alloc(K_STD_LIMIT), 0};
  struct curl_slist *headers = NULL;
#if LIBCURL_VERSION_NUM >= 0x075500
  curl_easy_setopt(curl, CURLOPT_PROTOCOLS_STR, "http,https");
#else
  curl_easy_setopt(curl, CURLOPT_PROTOCOLS,
                   (long)(CURLPROTO_HTTP | CURLPROTO_HTTPS));
#endif
  curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, k_curl_write);
  curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buffer);
  if (post) {
    headers = curl_slist_append(headers, "Content-Type: application/json");
    if (token.len) {
      KText h = k_concat(K_TEXT("Authorization: Bearer "), token);
      struct curl_slist *next = curl_slist_append(headers, h.ptr);
      k_drop(&h);
      if (!next)
        k_fail("allocation_failed", 0);
      headers = next;
    }
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body.ptr);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE_LARGE, (curl_off_t)body.len);
  }
  CURLcode code = curl_easy_perform(curl);
  long status = 0;
  curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
  curl_slist_free_all(headers);
  curl_easy_cleanup(curl);
  k_drop(&address);
  buffer.text.len = buffer.used;
  ((char *)buffer.text.ptr)[buffer.used] = 0;
  if (code != CURLE_OK || !k_utf8(buffer.text)) {
    k_drop(&buffer.text);
    return k_text_error(
        "HTTP transfer failed, exceeded limit, or returned non-UTF-8");
  }
  KText response = k_response(status, buffer.text);
  k_drop(&buffer.text);
  return k_text_ok(response);
}
static KTextResult k_http_get(KText url) {
  return k_http_request(url, K_TEXT(""), K_TEXT(""), false, 10000);
}
static KTextResult k_http_post_json(KText url, KText body, KText token) {
  return k_http_request(url, body, token, true, 10000);
}
static KTextResult k_http_post_json_timeout(KText url, KText body, KText token, int64_t timeout_ms) {
  return k_http_request(url, body, token, true, timeout_ms);
}
/* One text message each way. libcurl owns masking, TLS and the upgrade
 * handshake. */
static KTextResult k_websocket_exchange(KText url, KText message) {
  if (!k_url_allowed(url, true))
    return k_text_error("invalid WebSocket URL");
  if (message.len > K_STD_LIMIT)
    return k_text_error("message too large");
#if LIBCURL_VERSION_NUM >= 0x081000
  if (curl_version_info(CURLVERSION_NOW)->version_num < 0x081000)
    return k_text_error("WebSocket requires libcurl 8.16 or newer");
  KText address = k_clone(url);
  CURL *curl = k_curl(address);
  if (!curl) {
    k_drop(&address);
    return k_text_error("WebSocket initialization failed");
  }
  curl_easy_setopt(curl, CURLOPT_PROTOCOLS_STR, "ws,wss");
  curl_easy_setopt(curl, CURLOPT_CONNECT_ONLY, 2L);
  /* Consume handshake response bodies within the same bound, never stdout. */
  KCurlBuffer buffer = {k_alloc(K_STD_LIMIT), 0};
  curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, k_curl_write);
  curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buffer);
  int64_t deadline = k_millis() + 10000;
  CURLcode code = curl_easy_perform(curl);
  curl_socket_t socket = CURL_SOCKET_BAD;
  curl_easy_getinfo(curl, CURLINFO_ACTIVESOCKET, &socket);
  if (code != CURLE_OK) {
    k_drop(&buffer.text);
    curl_easy_cleanup(curl);
    k_drop(&address);
    return k_text_error("WebSocket unavailable or handshake failed");
  }
  size_t sent = 0;
  do {
    size_t n = 0;
    code = curl_ws_send(curl, message.ptr + sent, message.len - sent, &n, 0,
                        CURLWS_TEXT);
    sent += n;
    if (code == CURLE_AGAIN) {
      if (!k_wait_fd(socket, POLLOUT, deadline))
        goto ws_error;
    } else if (code != CURLE_OK)
      goto ws_error;
    if (k_millis() >= deadline)
      goto ws_error;
  } while (sent < message.len);
  buffer.used = 0;
  for (;;) {
    if (k_millis() >= deadline)
      goto ws_error;
    char chunk[16384];
    size_t n = 0;
    const struct curl_ws_frame *meta = NULL;
    code = curl_ws_recv(curl, chunk, sizeof(chunk), &n, &meta);
    if (code == CURLE_AGAIN) {
      if (!k_wait_fd(socket, POLLIN, deadline))
        goto ws_error;
      continue;
    }
    if (code != CURLE_OK)
      goto ws_error;
    if (meta->flags & CURLWS_CLOSE)
      goto ws_error;
    if (meta->flags & (CURLWS_PING | CURLWS_PONG))
      continue;
    if (!(meta->flags & CURLWS_TEXT) || n > K_STD_LIMIT - buffer.used)
      goto ws_error;
    memcpy((char *)buffer.text.ptr + buffer.used, chunk, n);
    buffer.used += n;
    if (meta->bytesleft == 0 && !(meta->flags & CURLWS_CONT))
      break;
  }
  buffer.text.len = buffer.used;
  ((char *)buffer.text.ptr)[buffer.used] = 0;
  curl_easy_cleanup(curl);
  k_drop(&address);
  if (!k_utf8(buffer.text)) {
    k_drop(&buffer.text);
    return k_text_error("response is not UTF-8");
  }
  return k_text_ok(buffer.text);
ws_error:
  k_drop(&buffer.text);
  curl_easy_cleanup(curl);
  k_drop(&address);
  return k_text_error("WebSocket exchange failed or timed out");
#else
  return k_text_error("WebSocket requires libcurl 8.16 or newer");
#endif
}
#endif
#ifdef KEEL_XML
#include <libxml/parser.h>
#include <libxml/tree.h>
static bool k_xml_depth(xmlNodePtr node, unsigned depth) {
  for (; node; node = node->next) {
    if (node->type != XML_ELEMENT_NODE)
      continue;
    if (depth > 64 || !k_xml_depth(node->children, depth + 1))
      return false;
  }
  return true;
}
static KTextResult k_xml_text(KText source, KText path) {
  if (source.len > K_STD_LIMIT || !k_utf8(source) || !k_cstr(source) ||
      !k_cstr(path) || path.len > K_STD_LIMIT)
    return k_text_error("invalid XML");
  /* Reject DTDs before parsing: no external subsets, entity declarations or
   * expansion. */
  for (size_t i = 0; i + 9 <= source.len; i++)
    if (!memcmp(source.ptr + i, "<!DOCTYPE", 9))
      return k_text_error("XML DTDs are disabled");
  xmlParserCtxtPtr context = xmlNewParserCtxt();
  if (!context)
    k_fail("allocation_failed", 0);
  xmlDocPtr doc = xmlCtxtReadMemory(
      context, source.ptr, (int)source.len, NULL, "UTF-8",
      XML_PARSE_NONET | XML_PARSE_NOERROR | XML_PARSE_NOWARNING);
  bool valid = doc && context->wellFormed && context->nsWellFormed;
  xmlFreeParserCtxt(context);
  if (!valid || (doc && !k_xml_depth(xmlDocGetRootElement(doc), 1))) {
    if (doc)
      xmlFreeDoc(doc);
    return k_text_error("invalid XML");
  }
  xmlNodePtr node = xmlDocGetRootElement(doc);
  size_t p = 0;
  if (!path.len || path.ptr[0] != '/') {
    xmlFreeDoc(doc);
    return k_text_error("invalid XML path");
  }
  while (p < path.len) {
    size_t start = ++p;
    while (p < path.len && path.ptr[p] != '/')
      p++;
    if (start == p) {
      xmlFreeDoc(doc);
      return k_text_error("invalid XML path");
    }
    while (node && (node->type != XML_ELEMENT_NODE || node->ns ||
                    strlen((char *)node->name) != p - start ||
                    memcmp(node->name, path.ptr + start, p - start)))
      node = node->next;
    if (!node) {
      xmlFreeDoc(doc);
      return k_text_error("XML path not found");
    }
    if (p < path.len)
      node = node->children;
  }
  xmlChar *text = xmlNodeGetContent(node);
  KText result =
      k_clone((KText){(char *)text, text ? strlen((char *)text) : 0, false});
  xmlFree(text);
  xmlFreeDoc(doc);
  return k_text_ok(result);
}
#endif
static KText k_trim(KText s) {
  size_t a = 0, b = s.len;
  while (a < b && (s.ptr[a] == ' ' || s.ptr[a] == '\t' || s.ptr[a] == '\r'))
    a++;
  while (b > a &&
         (s.ptr[b - 1] == ' ' || s.ptr[b - 1] == '\t' || s.ptr[b - 1] == '\r'))
    b--;
  return k_slice(s, a, b);
}
static KTextResult k_dotenv_get(KText source, KText name) {
  if (source.len > K_STD_LIMIT)
    return k_text_error("invalid dotenv");
  KText found = {0};
  for (size_t p = 0; p < source.len;) {
    size_t a = p;
    while (p < source.len && source.ptr[p] != '\n')
      p++;
    KText line = k_trim(k_slice(source, a, p));
    if (p < source.len)
      p++;
    if (!line.len || line.ptr[0] == '#')
      continue;
    size_t eq = 0;
    while (eq < line.len && line.ptr[eq] != '=')
      eq++;
    if (eq == line.len)
      return k_text_error("invalid dotenv");
    KText key = k_trim(k_slice(line, 0, eq)),
          value = k_trim(k_slice(line, eq + 1, line.len));
    if (!key.len)
      return k_text_error("invalid dotenv");
    for (size_t i = 0; i < key.len; i++) {
      char c = key.ptr[i];
      if (c != '_' && !(c >= 'a' && c <= 'z') && !(c >= 'A' && c <= 'Z') &&
          !(i > 0 && c >= '0' && c <= '9'))
        return k_text_error("invalid dotenv");
    }
    if (value.len && (value.ptr[0] == '\'' || value.ptr[0] == '"')) {
      if (value.len < 2 || value.ptr[value.len - 1] != value.ptr[0])
        return k_text_error("invalid dotenv");
      value = k_slice(value, 1, value.len - 1);
    }
    if (k_equal(key, name))
      found = value;
  }
  return found.ptr ? k_text_ok(k_clone(found))
                   : k_text_error("dotenv key not found");
}

static void k_std_init(void) {
#ifdef KEEL_XML
  xmlInitParser();
#endif
}
