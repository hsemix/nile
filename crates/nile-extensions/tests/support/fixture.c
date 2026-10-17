#include "nile_extension.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#ifdef _WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif
static NileExtensionApi host_api;
static uint64_t pending_context, pending_call;
static int32_t duplicate_status = -1, late_status = -1;
static uint32_t destroyed = 0, shutdown_count = 0;
static NileSlice str(const char *s) { NileSlice v = {(const uint8_t*)s, (uint64_t)strlen(s)}; return v; }
static int32_t reply(uint64_t context, uint64_t call, const char *json) {
  NileReply r = {0}; r.struct_size = sizeof(r); r.json = str(json);
  return host_api.complete(host_api.host, context, call, &r);
}
static void drop_resource(uint64_t payload) { destroyed += (uint32_t)payload; }
static NileStatus dispatch(uint64_t user, const NileRequest *r) {
  (void)user;
  if (r->operation.len == strlen("fixture.flood") && !memcmp(r->operation.data, "fixture.flood", r->operation.len)) {
    NileReply event = {0}; event.struct_size = sizeof(event); event.json = str("42");
    unsigned overloaded = 0;
    for (unsigned i = 0; i < 256; ++i) if (host_api.emit(host_api.host, r->context, r->call, &event) == NILE_OVERLOADED) ++overloaded;
    char output[32]; snprintf(output, sizeof(output), "%u", overloaded);
    return reply(r->context, r->call, output);
  }
  if (r->operation.len == strlen("fixture.pending") && !memcmp(r->operation.data, "fixture.pending", r->operation.len)) {
    pending_context = r->context; pending_call = r->call; return NILE_OK;
  }
  if (r->operation.len == strlen("fixture.error") && !memcmp(r->operation.data, "fixture.error", r->operation.len)) return NILE_FAILED;
  if (r->operation.len == strlen("fixture.resource") && !memcmp(r->operation.data, "fixture.resource", r->operation.len)) {
    uint64_t id = 0;
    NileStatus status = host_api.resource_create(host_api.host, r->context, 1, drop_resource, &id);
    char output[64];
    if (status != NILE_OK) return status;
    snprintf(output, sizeof(output), "\"%llu\"", (unsigned long long)id);
    return reply(r->context, r->call, output);
  }
  if ((r->operation.len == strlen("fixture.get") && !memcmp(r->operation.data, "fixture.get", r->operation.len)) ||
      (r->operation.len == strlen("fixture.release") && !memcmp(r->operation.data, "fixture.release", r->operation.len))) {
    char number[64] = {0}; uint64_t payload = 0;
    size_t n = r->json.len < 63 ? (size_t)r->json.len : 63;
    memcpy(number, r->json.data, n);
    uint64_t id = (uint64_t)strtoull(number + (number[0] == '"'), NULL, 10);
    NileStatus status = r->operation.len == strlen("fixture.get") ? host_api.resource_get(host_api.host, r->context, id, &payload) : host_api.resource_release(host_api.host, r->context, id);
    char output[32]; snprintf(output, sizeof(output), "%d", status);
    return reply(r->context, r->call, output);
  }
  NileStatus first = reply(r->context, r->call, "{\"answer\":42}");
  duplicate_status = reply(r->context, r->call, "null");
  return first;
}
static NileStatus dispose(uint64_t user, uint64_t context) {
  (void)user; (void)context;
  if (pending_call) late_status = reply(pending_context, pending_call, "null");
  return NILE_OK;
}
static NileStatus shutdown_extension(uint64_t user) { (void)user; ++shutdown_count; return NILE_OK; }
#ifdef FAIL_CONTEXT
static NileStatus init_context(uint64_t user, uint64_t context) {
  (void)user; uint64_t id;
  if (host_api.resource_create(host_api.host, context, 1, drop_resource, &id) != NILE_OK) return NILE_INVALID;
  return NILE_FAILED;
}
#endif
EXPORT int32_t fixture_duplicate_status(void) { return duplicate_status; }
EXPORT int32_t fixture_late_status(void) { return late_status; }
EXPORT int32_t fixture_complete_late(void) { return reply(pending_context, pending_call, "null"); }
EXPORT uint32_t fixture_destroyed(void) { return destroyed; }
EXPORT uint32_t fixture_shutdown_count(void) { return shutdown_count; }
#ifndef NO_ENTRY
EXPORT NileStatus nile_extension_init(const NileExtensionApi *api, NileExtension *extension) {
  host_api = *api;
  extension->abi_version = NILE_ABI_VERSION;
  extension->struct_size = sizeof(*extension);
  extension->context_dispose = dispose;
  extension->shutdown = shutdown_extension;
#ifdef FAIL_CONTEXT
  extension->context_init = init_context;
#endif
#ifdef BAD_TABLE
  extension->abi_version = 99;
  return NILE_OK;
#endif
  const char *ops[] = {"fixture.immediate", "fixture.pending", "fixture.error", "fixture.resource", "fixture.get", "fixture.release", "fixture.flood"};
  for (unsigned i = 0; i < sizeof(ops)/sizeof(ops[0]); ++i) {
    NileStatus s = api->register_operation(api->host, str(ops[i]), dispatch, 0);
    if (s != NILE_OK) return s;
  }
  return NILE_OK;
}
#endif
