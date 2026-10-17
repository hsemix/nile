/* Native dispatch fixture for the real SQLite JS wrapper. Counts actual callback
 * invocations and close side effects, independently of Promise observations. */
#include "nile_extension.h"
#include <stdio.h>
#include <string.h>
#ifdef _WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif
static NileExtensionApi api;
static unsigned live, closes, executions, queries, mode;
static NileSlice str(const char *s) { NileSlice v = {(const uint8_t*)s, (uint64_t)strlen(s)}; return v; }
static int equal(NileSlice s, const char *t) { return s.len == strlen(t) && !memcmp(s.data, t, (size_t)s.len); }
static int contains(NileSlice s, const char *t) {
  size_t n = strlen(t);
  for (uint64_t i = 0; i + n <= s.len; ++i) if (!memcmp(s.data + i, t, n)) return 1;
  return 0;
}
static NileStatus reply(const NileRequest *r, NileStatus status, const char *code, const char *json) {
  NileReply p = {0}; p.struct_size = sizeof(p); p.status = status;
  p.code = str(code); p.message = str(status ? "fixture failure" : ""); p.json = str(json);
  return api.complete(api.host, r->context, r->call, &p);
}
static NileStatus dispatch(uint64_t user, const NileRequest *r) {
  (void)user;
  if (equal(r->operation, "sqlite.open")) {
    live = 1; closes = executions = queries = 0;
    mode = contains(r->json, "ambiguous") ? 1 : contains(r->json, "operation") ? 2 : contains(r->json, "accepted") ? 3 : 0;
    return reply(r, NILE_OK, "", "{\"handle\":\"1\"}");
  }
  if (equal(r->operation, "sqlite.close")) {
    ++closes;
    if (!mode && closes == 1) return NILE_OVERLOADED; /* No work accepted. */
    live = 0; ++executions;
    if (mode == 1) return NILE_FAILED; /* Closed, but terminal response was lost. */
    if (mode == 2) return reply(r, NILE_OVERLOADED, "DISPATCH_4", "null"); /* Operation failure, not dispatch rejection. */
    NileStatus status = reply(r, NILE_OK, "", "null");
    return mode == 3 ? NILE_OVERLOADED : status; /* Accepted response wins. */
  }
  if (equal(r->operation, "sqlite.query")) {
    ++queries;
    if (!live) return reply(r, NILE_FAILED, "SQLITE_CLOSED", "null");
    return reply(r, NILE_OK, "", "[{\"answer\":42}]");
  }
  char json[160];
  snprintf(json, sizeof(json), "{\"open\":%u,\"closeDispatches\":%u,\"closeExecutions\":%u,\"queryDispatches\":%u}", live, closes, executions, queries);
  return reply(r, NILE_OK, "", json);
}
static NileStatus dispose(uint64_t user, uint64_t context) { (void)user; (void)context; live = 0; return NILE_OK; }
static NileStatus shutdown_extension(uint64_t user) { (void)user; return NILE_OK; }
EXPORT NileStatus nile_extension_init(const NileExtensionApi *host, NileExtension *extension) {
  api = *host;
  extension->abi_version = NILE_ABI_VERSION; extension->struct_size = sizeof(*extension);
  extension->context_dispose = dispose; extension->shutdown = shutdown_extension;
  const char *ops[] = {"sqlite.open", "sqlite.query", "sqlite.close", "sqlite.stats"};
  for (unsigned i = 0; i < sizeof(ops)/sizeof(ops[0]); ++i) {
    NileStatus status = api.register_operation(api.host, str(ops[i]), dispatch, 0);
    if (status != NILE_OK) return status;
  }
  return NILE_OK;
}
