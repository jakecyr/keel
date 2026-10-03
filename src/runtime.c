/* Trusted v0 host runtime. POSIX + GCC/Clang overflow builtins. */
#define _POSIX_C_SOURCE 200809L
#include <stdbool.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <errno.h>
#include <signal.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <netinet/in.h>

typedef struct { const char *ptr; size_t len; bool owned; } KText;
static bool k_has_value = false;
static int64_t k_case_value = 0;
static const char *k_net_permission = NULL;
static bool k_stdout_permission = false;
static void k_fail(const char *kind, size_t offset) {
    fprintf(stderr, "{\"kind\":\"%s\",\"offset\":%zu,\"has_value\":%s,\"value\":%" PRId64 "}\n", kind, offset, k_has_value ? "true" : "false", k_case_value);
    exit(strcmp(kind, "hole_reached") == 0 ? 3 : 1);
}
static void k_drop(KText *text) { if (text->owned) free((void *)text->ptr); *text = (KText){0}; }
static KText k_move(KText *text) { KText result = *text; *text = (KText){0}; return result; }
static KText k_alloc(size_t len) {
    if (len == SIZE_MAX) k_fail("allocation_limit", 0);
    char *ptr = malloc(len + 1); if (!ptr) k_fail("allocation_failed", 0); ptr[len] = 0;
    return (KText){ptr, len, true};
}
static KText k_clone(KText input) { KText out = k_alloc(input.len); if (input.len) memcpy((void *)out.ptr, input.ptr, input.len); return out; }
static KText k_concat(KText a, KText b) {
    if (a.len > SIZE_MAX - b.len) k_fail("allocation_limit", 0);
    KText out = k_alloc(a.len + b.len);
    if (a.len) memcpy((void *)out.ptr, a.ptr, a.len);
    if (b.len) memcpy((char *)out.ptr + a.len, b.ptr, b.len); return out;
}
static bool k_equal(KText a, KText b) { return a.len == b.len && (!a.len || memcmp(a.ptr, b.ptr, a.len) == 0); }
static int64_t k_len(KText text) { if (text.len > INT64_MAX) k_fail("overflow", 0); return (int64_t)text.len; }
static KText k_from_int(int64_t value) { char buf[32]; int n = snprintf(buf, sizeof(buf), "%" PRId64, value); return k_clone((KText){buf, (size_t)n, false}); }
static void k_println(KText text) { if (!k_stdout_permission) k_fail("permission_denied_stdout", 0); fwrite(text.ptr, 1, text.len, stdout); putchar('\n'); }
static int64_t k_add(int64_t a, int64_t b, size_t at) { int64_t out; if (__builtin_add_overflow(a,b,&out)) k_fail("overflow", at); return out; }
static int64_t k_sub(int64_t a, int64_t b, size_t at) { int64_t out; if (__builtin_sub_overflow(a,b,&out)) k_fail("overflow", at); return out; }
static int64_t k_mul(int64_t a, int64_t b, size_t at) { int64_t out; if (__builtin_mul_overflow(a,b,&out)) k_fail("overflow", at); return out; }
static int64_t k_div(int64_t a, int64_t b, size_t at) { if (!b) k_fail("division_by_zero", at); if (a == INT64_MIN && b == -1) k_fail("overflow", at); return a / b; }
static int64_t k_rem(int64_t a, int64_t b, size_t at) { if (!b) k_fail("division_by_zero", at); if (a == INT64_MIN && b == -1) k_fail("overflow", at); return a % b; }
/* Explicit owned containers; no implicit cloning. */
typedef struct { int64_t *ptr; size_t len; size_t cap; } KList;
typedef struct { bool some; int64_t value; } KOption;
typedef struct { bool ok; int64_t value; KText error; } KResult;
static KList k_list_new(void) { return (KList){0}; }
static void k_list_drop(KList *list) { free(list->ptr); *list = (KList){0}; }
static KList k_list_move(KList *list) { KList out = *list; *list = (KList){0}; return out; }
static void k_list_push(KList *list, int64_t value) {
    if (list->len >= INT64_MAX || list->len >= SIZE_MAX / sizeof(int64_t)) k_fail("allocation_limit", 0);
    if (list->len == list->cap) {
        size_t limit = SIZE_MAX / sizeof(int64_t);
        size_t cap = list->cap > limit / 2 ? limit : (list->cap ? list->cap * 2 : 8);
        int64_t *ptr = realloc(list->ptr, cap * sizeof(int64_t));
        if (!ptr) k_fail("allocation_failed", 0);
        list->ptr = ptr; list->cap = cap;
    }
    list->ptr[list->len++] = value;
}
static KList k_list_clone(KList list) {
    KList out = {0};
    if (list.len) {
        out.ptr = malloc(list.len * sizeof(int64_t));
        if (!out.ptr) k_fail("allocation_failed", 0);
        memcpy(out.ptr, list.ptr, list.len * sizeof(int64_t)); out.len = list.len; out.cap = list.len;
    }
    return out;
}
static int64_t k_list_len(KList list) { return (int64_t)list.len; }
static KOption k_some(int64_t value) { return (KOption){true, value}; }
static KOption k_none(void) { return (KOption){0}; }
static KOption k_list_get(KList list, int64_t index) { return index >= 0 && (uint64_t)index < list.len ? k_some(list.ptr[index]) : k_none(); }
static int64_t k_list_at(KList list, int64_t index, size_t at) { if (index < 0 || (uint64_t)index >= list.len) k_fail("bounds", at); return list.ptr[index]; }
static void k_list_set(KList *list, int64_t index, int64_t value, size_t at) { if (index < 0 || (uint64_t)index >= list->len) k_fail("bounds", at); list->ptr[index] = value; }
static bool k_list_contains(KList list, int64_t value) { for (size_t i=0; i<list.len; i++) if (list.ptr[i] == value) return true; return false; }
static bool k_list_equal(KList a, KList b) { return a.len == b.len && (!a.len || memcmp(a.ptr, b.ptr, a.len * sizeof(int64_t)) == 0); }
static bool k_option_equal(KOption a, KOption b) { return a.some == b.some && (!a.some || a.value == b.value); }
static KResult k_ok(int64_t value) { return (KResult){true, value, {0}}; }
static KResult k_err(KText error) { return (KResult){false, 0, error}; }
static void k_result_drop(KResult *result) { k_drop(&result->error); *result = (KResult){0}; }
static KResult k_result_move(KResult *result) { KResult out = *result; *result = (KResult){0}; return out; }
static bool k_result_equal(KResult a, KResult b) { return a.ok == b.ok && (a.ok ? a.value == b.value : k_equal(a.error, b.error)); }
static KResult k_parse_int(KText text) {
    if (!text.len) return k_err((KText){"empty integer", 13, false});
    size_t i = 0; bool negative = text.ptr[0] == '-';
    if (negative) i++;
    if (i == text.len) return k_err((KText){"invalid integer", 15, false});
    uint64_t magnitude = 0, limit = negative ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    for (; i < text.len; i++) {
        unsigned char c = (unsigned char)text.ptr[i];
        if (c < '0' || c > '9') return k_err((KText){"invalid integer", 15, false});
        unsigned digit = c - '0';
        if (magnitude > (limit - digit) / 10) return k_err((KText){"integer out of range", 20, false});
        magnitude = magnitude * 10 + digit;
    }
    return k_ok(negative ? (magnitude == (uint64_t)INT64_MAX + 1 ? INT64_MIN : -(int64_t)magnitude) : (int64_t)magnitude);
}
#define k_drop(value) _Generic((value), KText*: k_drop, KList*: k_list_drop, KResult*: k_result_drop)(value)
#define k_move(value) _Generic((value), KText*: k_move, KList*: k_list_move, KResult*: k_result_move)(value)
static KText k_response(int64_t status, KText body) {
    if (status < 100 || status > 599) k_fail("invalid_http_status", 0);
    const char *reason = status == 200 ? "OK" : status == 404 ? "Not Found" : status == 405 ? "Method Not Allowed" : "Response";
    char header[256]; int n = snprintf(header, sizeof(header), "HTTP/1.1 %" PRId64 " %s\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n", status, reason, body.len);
    return k_concat((KText){header, (size_t)n, false}, body);
}
static int64_t k_status(KText response) {
    if (response.len < 12 || memcmp(response.ptr,"HTTP/1.1 ",9)) return 0;
    int64_t out = 0; for (size_t i=9; i<12; i++) { if (response.ptr[i] < '0' || response.ptr[i] > '9') return 0; out = out*10 + response.ptr[i]-'0'; } return out;
}
static KText k_body(KText response) {
    for (size_t i=0; i+4<=response.len; i++) if (memcmp(response.ptr+i,"\r\n\r\n",4)==0) return k_clone((KText){response.ptr+i+4,response.len-i-4,false});
    return k_alloc(0);
}
static bool k_send_all(int socket, KText data) {
    size_t sent = 0;
    while (sent < data.len) { ssize_t n = send(socket,data.ptr+sent,data.len-sent,0); if (n<0 && errno==EINTR) continue; if (n<=0) return false; sent += (size_t)n; } return true;
}
static void k_serve(int64_t port, KText (*handler)(KText)) {
    if (port < 1 || port > 65535) k_fail("invalid_port",0);
    char permission[64]; snprintf(permission,sizeof(permission),"127.0.0.1:%" PRId64,port);
    if (!k_net_permission || strcmp(permission,k_net_permission)) k_fail("permission_denied_net",0);
    signal(SIGPIPE,SIG_IGN);
    int server = socket(AF_INET,SOCK_STREAM,0); if (server<0) k_fail("socket_failed",0);
    int reuse = 1; setsockopt(server,SOL_SOCKET,SO_REUSEADDR,&reuse,sizeof(reuse));
    struct sockaddr_in addr = {0}; addr.sin_family=AF_INET; addr.sin_addr.s_addr=htonl(UINT32_C(0x7f000001)); addr.sin_port=htons((uint16_t)port);
    if (bind(server,(struct sockaddr *)&addr,sizeof(addr))<0 || listen(server,16)<0) { close(server); k_fail("listen_failed",0); }
    fprintf(stderr,"Keel listening on http://127.0.0.1:%" PRId64 "\n",port); fflush(stderr);
    for (;;) {
        int client = accept(server,NULL,NULL); if (client<0) { if(errno==EINTR) continue; close(server); k_fail("accept_failed",0); }
        struct timeval timeout = {2,0}; setsockopt(client,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout)); setsockopt(client,SOL_SOCKET,SO_SNDTIMEO,&timeout,sizeof(timeout));
        char request[16385]; size_t used=0; bool complete=false;
        while (used < sizeof(request)-1) {
            ssize_t n=recv(client,request+used,sizeof(request)-1-used,0);
            if(n<0 && errno==EINTR) continue;
            if(n<=0) break;
            if(memchr(request+used,0,(size_t)n)) break;
            used+=(size_t)n; request[used]=0;
            if(strstr(request,"\r\n\r\n")) { complete=true; break; }
        }
        KText response={0};
        if (!complete) response=k_response(400,(KText){"incomplete or oversized request\n",32,false});
        else {
            char *space=memchr(request,' ',used); char *line=strstr(request,"\r\n");
            char *end=space ? memchr(space+1,' ',used-(size_t)(space+1-request)) : NULL;
            bool valid=space && end && line && end<line && end>space+1 && space[1]=='/' && (size_t)(line-end)==9 && (!memcmp(end+1,"HTTP/1.1",8) || !memcmp(end+1,"HTTP/1.0",8));
            if(!valid) response=k_response(400,(KText){"bad request\n",12,false});
            else if(space-request!=3 || memcmp(request,"GET",3)) response=k_response(405,(KText){"GET only\n",9,false});
            else { char *query=memchr(space+1,'?',(size_t)(end-space-1)); if(query) end=query; response=handler((KText){space+1,(size_t)(end-space-1),false}); }
        }
        k_send_all(client,response); k_drop(&response); close(client);
    }
}
static uint64_t k_random(uint64_t *state) { uint64_t x=*state; x^=x<<13; x^=x>>7; x^=x<<17; return *state=x; }
static int64_t k_generate(uint64_t *state, size_t i, int64_t min, int64_t max) {
    if(i==0) return min; if(i==1) return max;
    int64_t edges[3]={0,-1,1}; if(i>=2 && i<5 && edges[i-2]>=min && edges[i-2]<=max) return edges[i-2];
    __uint128_t width=(__uint128_t)((__int128)max-(__int128)min)+1;
    return (int64_t)((__int128)min+(__int128)((__uint128_t)k_random(state)%width));
}
