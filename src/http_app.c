/* Experimental sequential localhost application host. Binary assets stay in
 * this trusted adapter; they are never exposed as language Text values. */
#include <strings.h>
#define K_APP_HEADERS 16384u
#define K_APP_FILE_LIMIT (8u * 1024u * 1024u)

static bool k_app_send(int fd, const char *data, size_t size, int64_t deadline) {
  size_t sent = 0;
  while (sent < size) {
    if (!k_wait_fd(fd, POLLOUT, deadline)) return false;
    ssize_t n = send(fd, data + sent, size - sent, 0);
    if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
    if (n <= 0) return false;
    sent += (size_t)n;
  }
  return true;
}
static void k_app_response(int fd, KText response, bool head, int64_t deadline) {
  size_t size = response.len;
  if (head) {
    for (size_t i = 0; i + 4 <= response.len; i++) {
      if (!memcmp(response.ptr + i, "\r\n\r\n", 4)) { size = i + 4; break; }
    }
  }
  k_app_send(fd, response.ptr, size, deadline);
}
static void k_app_error(int fd, int status, const char *message, bool head) {
  KText response = k_response(status, (KText){message, strlen(message), false});
  k_app_response(fd, response, head, k_millis() + 2000);
  k_drop(&response);
}
static bool k_app_token(unsigned char c) {
  return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
         (c >= '0' && c <= '9') || (c && strchr("!#$%&'*+-.^_`|~", c));
}
static KText k_http_path(KText target) {
  const char *query = memchr(target.ptr, '?', target.len);
  return k_clone((KText){target.ptr, query ? (size_t)(query - target.ptr) : target.len, false});
}
static bool k_http_name_equal(KText a, KText b) {
  if (a.len != b.len) return false;
  for (size_t i = 0; i < a.len; i++) {
    unsigned char x = (unsigned char)a.ptr[i], y = (unsigned char)b.ptr[i];
    if (x >= 'A' && x <= 'Z') x += 'a' - 'A';
    if (y >= 'A' && y <= 'Z') y += 'a' - 'A';
    if (x != y) return false;
  }
  return true;
}
static KTextResult k_http_header(KText headers, KText name) {
  if (headers.len > K_APP_HEADERS || !name.len || !k_utf8(headers)) return k_text_error("invalid HTTP headers");
  for (size_t i = 0; i < name.len; i++) if (!k_app_token((unsigned char)name.ptr[i])) return k_text_error("invalid HTTP headers");
  KText found = {0}; size_t count = 0;
  for (size_t p = 0; p < headers.len;) {
    size_t end = p;
    while (end + 1 < headers.len && memcmp(headers.ptr + end, "\r\n", 2)) end++;
    if (end + 1 >= headers.len) return k_text_error("invalid HTTP headers");
    size_t colon = p;
    while (colon < end && headers.ptr[colon] != ':') {
      if (!k_app_token((unsigned char)headers.ptr[colon])) return k_text_error("invalid HTTP headers");
      colon++;
    }
    if (colon == p || colon == end) return k_text_error("invalid HTTP headers");
    size_t a = colon + 1, b = end;
    for (size_t i = a; i < b; i++) {
      unsigned char c = (unsigned char)headers.ptr[i];
      if ((c < 32 && c != '\t') || c == 127) return k_text_error("invalid HTTP headers");
    }
    while (a < b && (headers.ptr[a] == ' ' || headers.ptr[a] == '\t')) a++;
    while (b > a && (headers.ptr[b - 1] == ' ' || headers.ptr[b - 1] == '\t')) b--;
    if (k_http_name_equal(k_slice(headers, p, colon), name)) { count++; found = k_slice(headers, a, b); }
    p = end + 2;
  }
  if (count > 1) return k_text_error("duplicate HTTP header");
  return count ? k_text_ok(k_clone(found)) : k_text_error("HTTP header not found");
}
static KTextResult k_form_decode(KText input) {
  KText out = k_alloc(input.len); size_t n = 0;
  for (size_t i = 0; i < input.len; i++) {
    unsigned char c = (unsigned char)input.ptr[i];
    if (c == '+') c = ' ';
    else if (c == '%') {
      if (i + 2 >= input.len) goto invalid;
      int a = k_hex(input.ptr[i + 1]), b = k_hex(input.ptr[i + 2]);
      if (a < 0 || b < 0) goto invalid;
      c = (unsigned char)(a * 16 + b); i += 2;
    }
    if (c < 32 || c == 127) goto invalid;
    ((char *)out.ptr)[n++] = (char)c;
  }
  out.len = n; ((char *)out.ptr)[n] = 0;
  if (!k_utf8(out)) goto invalid;
  return k_text_ok(out);
invalid:
  k_drop(&out); return k_text_error("invalid HTTP query");
}
static KTextResult k_http_query(KText target, KText name) {
  if (target.len > 4096 || !target.len || target.ptr[0] != '/' || !name.len || name.len > 4096 || !k_utf8(target)) return k_text_error("invalid HTTP query");
  size_t query = target.len;
  for (size_t i = 0; i < target.len; i++) {
    unsigned char c = (unsigned char)target.ptr[i];
    if (c <= 32 || c == 127 || c == '#') return k_text_error("invalid HTTP query");
    if (c == '?' && query == target.len) query = i;
  }
  KText found = {0}; size_t count = 0;
  for (size_t p = query < target.len ? query + 1 : target.len; p < target.len;) {
    size_t end = p;
    while (end < target.len && target.ptr[end] != '&') end++;
    if (end == p) { p++; continue; }
    size_t eq = p;
    while (eq < end && target.ptr[eq] != '=') eq++;
    KTextResult key = k_form_decode(k_slice(target, p, eq));
    KTextResult value = k_form_decode(k_slice(target, eq < end ? eq + 1 : end, end));
    if (!key.ok || !value.ok) {
      k_drop(&key); k_drop(&value); k_drop(&found); return k_text_error("invalid HTTP query");
    }
    if (k_equal(key.value, name)) { count++; k_drop(&found); found = k_move(&value.value); }
    k_drop(&key); k_drop(&value);
    p = end < target.len ? end + 1 : end;
  }
  if (count == 1) return k_text_ok(found);
  k_drop(&found);
  return k_text_error(count ? "duplicate HTTP query parameter" : "HTTP query parameter not found");
}
/* Parse exactly one framed request. Unsupported transfer encodings and duplicate
 * lengths are rejected rather than guessing which framing the client intended. */
static int k_app_request(int fd, int64_t port, KText *storage, KText *method, KText *path, KText *target, KText *headers, KText *body) {
  *storage = k_alloc(K_APP_HEADERS + K_STD_LIMIT);
  char *data = (char *)storage->ptr;
  size_t used = 0, header_size = 0, body_size = 0;
  int64_t deadline = k_millis() + 2000;
  while (!header_size) {
    if (used == K_APP_HEADERS) return 431;
    if (!k_wait_fd(fd, POLLIN, deadline)) return 408;
    ssize_t n = recv(fd, data + used, K_APP_HEADERS - used, 0);
    if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
    if (n <= 0) return 400;
    size_t start = used > 3 ? used - 3 : 0;
    used += (size_t)n;
    data[used] = 0;
    for (size_t i = start; i + 4 <= used; i++) {
      if (!memcmp(data + i, "\r\n\r\n", 4)) { header_size = i + 4; break; }
    }
  }
  if (memchr(data, 0, header_size)) return 400;
  char *line = strstr(data, "\r\n");
  if (!line) return 400;
  char *first = memchr(data, ' ', (size_t)(line - data));
  if (!first || first == data) return 400;
  char *second = memchr(first + 1, ' ', (size_t)(line - first - 1));
  if (!second || second == first + 1 || first[1] != '/' || line - second != 9 ||
      (memcmp(second + 1, "HTTP/1.1", 8) && memcmp(second + 1, "HTTP/1.0", 8))) return 400;
  for (char *p = data; p < first; p++) if (!k_app_token((unsigned char)*p)) return 400;
  for (char *p = first + 1; p < second; p++) if ((unsigned char)*p <= 32 || *p == 127) return 400;
  if (second - first > 4097) return 414;
  *method = (KText){data, (size_t)(first - data), false};
  char *query = memchr(first + 1, '?', (size_t)(second - first - 1));
  *path = (KText){first + 1, (size_t)((query ? query : second) - first - 1), false};
  *target = (KText){first + 1, (size_t)(second - first - 1), false};
  *headers = (KText){line + 2, (size_t)((data + header_size - 2) - (line + 2)), false};
  if (!k_utf8(*target) || !k_utf8(*headers) || memchr(target->ptr, '#', target->len)) return 400;
  bool has_length = false, has_host = false, has_origin = false, has_site = false;
  KText host = {0}, origin = {0}, site = {0};
  char *cursor = line + 2, *headers_end = data + header_size - 2;
  while (cursor < headers_end) {
    char *end = strstr(cursor, "\r\n");
    if (!end || end >= headers_end) return 400;
    char *colon = memchr(cursor, ':', (size_t)(end - cursor));
    if (!colon || colon == cursor) return 400;
    for (char *p = cursor; p < colon; p++) if (!k_app_token((unsigned char)*p)) return 400;
    char *value = colon + 1, *value_end = end;
    for (char *p = value; p < end; p++) if (((unsigned char)*p < 32 && *p != '\t') || *p == 127) return 400;
    while (value < end && (*value == ' ' || *value == '\t')) value++;
    while (value_end > value && (value_end[-1] == ' ' || value_end[-1] == '\t')) value_end--;
    size_t key_size = (size_t)(colon - cursor);
    if (key_size == 4 && !strncasecmp(cursor, "Host", 4)) {
      if (has_host) return 400;
      has_host = true; host = (KText){value, (size_t)(value_end - value), false};
    }
    if (key_size == 6 && !strncasecmp(cursor, "Origin", 6)) {
      if (has_origin) return 400;
      has_origin = true; origin = (KText){value, (size_t)(value_end - value), false};
    }
    if (key_size == 14 && !strncasecmp(cursor, "Sec-Fetch-Site", 14)) {
      if (has_site) return 400;
      has_site = true; site = (KText){value, (size_t)(value_end - value), false};
    }
    if (key_size == 17 && !strncasecmp(cursor, "Transfer-Encoding", 17)) return 400;
    if (key_size == 6 && !strncasecmp(cursor, "Expect", 6)) return 417;
    if (key_size == 14 && !strncasecmp(cursor, "Content-Length", 14)) {
      if (has_length || value == value_end) return 400;
      has_length = true;
      for (char *p = value; p < value_end; p++) {
        if (*p < '0' || *p > '9') return 400;
        if (body_size > (K_STD_LIMIT - (unsigned)(*p - '0')) / 10) return 413;
        body_size = body_size * 10 + (unsigned)(*p - '0');
      }
    }
    cursor = end + 2;
  }
  // This host is loopback-only. Reject rebinding hostnames and browser requests
  // from other origins before invoking a potentially effectful API handler.
  char localhost[64], loopback[64];
  snprintf(localhost, sizeof(localhost), "localhost:%" PRId64, port);
  snprintf(loopback, sizeof(loopback), "127.0.0.1:%" PRId64, port);
  bool local_host = k_equal(host, (KText){localhost, strlen(localhost), false}) ||
                    k_equal(host, (KText){loopback, strlen(loopback), false});
  if (port == 80) local_host = local_host || k_equal(host, K_TEXT("localhost")) || k_equal(host, K_TEXT("127.0.0.1"));
  if (!has_host || !local_host) return 403;
  if (has_origin) {
    KText expected = k_concat(K_TEXT("http://"), host);
    bool same = k_equal(origin, expected); k_drop(&expected);
    if (!same) return 403;
  }
  if (has_site && !k_equal(site, K_TEXT("same-origin")) && !k_equal(site, K_TEXT("none"))) return 403;
  size_t total = header_size + body_size;
  if (used > total) return 400; /* no pipelining or unframed body */
  while (used < total) {
    if (!k_wait_fd(fd, POLLIN, deadline)) return 408;
    ssize_t n = recv(fd, data + used, total - used, 0);
    if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
    if (n <= 0) return 400;
    used += (size_t)n;
  }
  storage->len = total;
  data[total] = 0;
  *body = (KText){data + header_size, body_size, false};
  if (!k_utf8(*body) || memchr(body->ptr, 0, body->len)) return 400;
  return 0;
}
static const char *k_app_mime(const char *name) {
  const char *ext = strrchr(name, '.');
  if (!ext) return "application/octet-stream";
  if (!strcasecmp(ext, ".html") || !strcasecmp(ext, ".htm")) return "text/html; charset=utf-8";
  if (!strcasecmp(ext, ".css")) return "text/css; charset=utf-8";
  if (!strcasecmp(ext, ".js") || !strcasecmp(ext, ".mjs")) return "text/javascript; charset=utf-8";
  if (!strcasecmp(ext, ".json")) return "application/json";
  if (!strcasecmp(ext, ".svg")) return "image/svg+xml";
  if (!strcasecmp(ext, ".png")) return "image/png";
  if (!strcasecmp(ext, ".jpg") || !strcasecmp(ext, ".jpeg")) return "image/jpeg";
  if (!strcasecmp(ext, ".gif")) return "image/gif";
  if (!strcasecmp(ext, ".webp")) return "image/webp";
  if (!strcasecmp(ext, ".ico")) return "image/x-icon";
  if (!strcasecmp(ext, ".woff")) return "font/woff";
  if (!strcasecmp(ext, ".woff2")) return "font/woff2";
  if (!strcasecmp(ext, ".wasm")) return "application/wasm";
  if (!strcasecmp(ext, ".pdf")) return "application/pdf";
  if (!strcasecmp(ext, ".txt")) return "text/plain; charset=utf-8";
  return "application/octet-stream";
}
/* Walk relative to an already-open root, without following any symlink. Dot
 * segments/files and encoded separators are denied. Never concatenate an
 * untrusted path with a filesystem root or send binary data through Keel Text. */
static bool k_app_static(int client, int root, KText path, bool head) {
  if (root < 0 || path.len < 1 || path.ptr[0] != '/' || path.len > 4096) return false;
  char name[4097]; size_t used = 0;
  for (size_t i = 1; i < path.len; i++) {
    unsigned char c = (unsigned char)path.ptr[i];
    if (c == '%') {
      if (i + 2 >= path.len) return false;
      int a = k_hex(path.ptr[i + 1]), b = k_hex(path.ptr[i + 2]);
      if (a < 0 || b < 0) return false;
      c = (unsigned char)(a * 16 + b); i += 2;
      if (c == '/' || c == '\\') return false;
    }
    if (c < 32 || c == 127 || c == '\\' || c == '#' || c == '?') return false;
    name[used++] = (char)c;
  }
  name[used] = 0;
  if (!k_utf8((KText){name, used, false})) return false;
  int fd = dup(root);
  if (fd < 0) return false;
  char *part = name;
  const char *filename = "index.html";
  while (*part) {
    char *slash = strchr(part, '/');
    if (slash) *slash = 0;
    if (!*part || part[0] == '.') { close(fd); return false; }
    int next = openat(fd, part, O_RDONLY | O_NONBLOCK | O_NOFOLLOW | (slash ? O_DIRECTORY : 0));
    close(fd); fd = next;
    if (fd < 0) return false;
    filename = part;
    if (!slash) break;
    part = slash + 1;
  }
  struct stat st;
  if (fstat(fd, &st)) { close(fd); return false; }
  if (S_ISDIR(st.st_mode)) {
    int index = openat(fd, "index.html", O_RDONLY | O_NONBLOCK | O_NOFOLLOW);
    close(fd); fd = index; filename = "index.html";
    if (fd < 0) return false;
    if (fstat(fd, &st)) { close(fd); return false; }
  }
  if (!S_ISREG(st.st_mode) || st.st_size < 0 || st.st_size > K_APP_FILE_LIMIT) { close(fd); return false; }
  char header[384];
  int n = snprintf(header, sizeof(header), "HTTP/1.1 200 OK\r\nContent-Type: %s\r\nContent-Length: %zu\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", k_app_mime(filename), (size_t)st.st_size);
  int64_t deadline = k_millis() + 2000;
  bool sent = k_app_send(client, header, (size_t)n, deadline);
  size_t remaining = (size_t)st.st_size;
  char buffer[16384];
  while (sent && !head && remaining) {
    ssize_t count = read(fd, buffer, remaining < sizeof(buffer) ? remaining : sizeof(buffer));
    if (count < 0 && errno == EINTR) continue;
    if (count <= 0) break;
    sent = k_app_send(client, buffer, (size_t)count, deadline);
    remaining -= (size_t)count;
  }
  close(fd);
  return true;
}
static void k_serve_application(int64_t port, KText static_root, KText (*handler)(KText, KText, KText), KText (*api_handler)(KText, KText, KText, KText)) {
  if (port < 1 || port > 65535) k_fail("invalid_port", 0);
  char permission[64]; snprintf(permission, sizeof(permission), "127.0.0.1:%" PRId64, port);
  if (!k_net_permission || strcmp(permission, k_net_permission)) k_fail("permission_denied_net", 0);
  int root = -1;
  if (static_root.len) {
    if (!k_allowed("--allow-read=", static_root)) k_fail("permission_denied_fs", 0);
    if (!k_cstr(static_root)) k_fail("invalid_static_root", 0);
    KText name = k_clone(static_root);
    root = open(name.ptr, O_RDONLY | O_DIRECTORY | O_NOFOLLOW);
    k_drop(&name);
    if (root < 0) k_fail("invalid_static_root", 0);
  }
  signal(SIGPIPE, SIG_IGN);
  int server = socket(AF_INET, SOCK_STREAM, 0);
  if (server < 0) k_fail("socket_failed", 0);
  int reuse = 1; setsockopt(server, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET; addr.sin_addr.s_addr = htonl(UINT32_C(0x7f000001)); addr.sin_port = htons((uint16_t)port);
  if (bind(server, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(server, 16) < 0) k_fail("listen_failed", 0);
  fprintf(stderr, "Keel app listening on http://127.0.0.1:%" PRId64 "\n", port); fflush(stderr);
  for (;;) {
    int client = accept(server, NULL, NULL);
    if (client < 0) { if (errno == EINTR) continue; k_fail("accept_failed", 0); }
    int flags = fcntl(client, F_GETFL, 0);
    if (flags < 0 || fcntl(client, F_SETFL, flags | O_NONBLOCK) < 0) { close(client); continue; }
    KText storage = {0}, method = {0}, path = {0}, target = {0}, headers = {0}, body = {0};
    int error = k_app_request(client, port, &storage, &method, &path, &target, &headers, &body);
    bool head = k_equal(method, K_TEXT("HEAD"));
    if (error) k_app_error(client, error, "invalid or unsupported request\n", head);
    else {
      KText response = api_handler ? api_handler(method, target, headers, body) : handler(method, path, body);
      if (!(k_status(response) == 404 && (head || k_equal(method, K_TEXT("GET"))) && k_app_static(client, root, path, head)))
        k_app_response(client, response, head, k_millis() + 2000);
      k_drop(&response);
    }
    k_drop(&storage); close(client);
  }
}

static void k_serve_app(int64_t port, KText root, KText (*handler)(KText, KText, KText)) {
  k_serve_application(port, root, handler, NULL);
}
static void k_serve_api(int64_t port, KText root, KText (*handler)(KText, KText, KText, KText)) {
  k_serve_application(port, root, NULL, handler);
}
