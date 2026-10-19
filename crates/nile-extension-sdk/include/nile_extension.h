#ifndef NILE_EXTENSION_H
#define NILE_EXTENSION_H

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
/* Marks the one symbol an extension library exports. */
#ifndef NILE_EXTENSION_EXPORT
#  if defined(_WIN32)
#    define NILE_EXTENSION_EXPORT __declspec(dllexport)
#  else
#    define NILE_EXTENSION_EXPORT __attribute__((visibility("default")))
#  endif
#endif


#define NILE_ABI_VERSION 1

#define NILE_CAP_ASYNC 1

#define NILE_CAP_BUFFERS 2

#define NILE_CAP_RESOURCES 4

#define NILE_CAP_EVENTS 8

#define NILE_CAP_CANCEL 16

#define NILE_CAP_ALL 31

#define NILE_SUBSCRIBE 1

typedef int32_t NileStatus;

typedef struct NileSlice {
  const uint8_t *data;
  uint64_t len;
} NileSlice;

typedef struct NileBuffers {
  const struct NileSlice *data;
  uint32_t len;
} NileBuffers;

typedef struct NileRequest {
  uint32_t struct_size;
  uint32_t flags;
  uint64_t context;
  uint64_t call;
  struct NileSlice operation;
  struct NileSlice json;
  struct NileBuffers buffers;
  /**
   * Absolute configuration directory, UTF-8. Independent of subsequent chdir calls.
   */
  struct NileSlice base_path;
} NileRequest;

/**
 * Returning NILE_OVERLOADED guarantees no work was accepted or executed. Other
 * failures do not guarantee that effects have not occurred. An accepted completion
 * takes precedence over a subsequent nonzero dispatch return.
 */
typedef NileStatus (*NileDispatch)(uint64_t user, const struct NileRequest *request);

typedef struct NileReply {
  uint32_t struct_size;
  NileStatus status;
  struct NileSlice code;
  struct NileSlice message;
  struct NileSlice json;
  struct NileBuffers buffers;
} NileReply;

typedef void (*NileResourceDrop)(uint64_t payload);

typedef struct NileExtensionApi {
  uint32_t abi_version;
  uint32_t struct_size;
  uint64_t capabilities;
  /**
   * Opaque host token, never a pointer. Invalid after shutdown; safe to pass on late completion.
   */
  uint64_t host;
  NileStatus (*register_operation)(uint64_t, struct NileSlice, NileDispatch, uint64_t);
  NileStatus (*complete)(uint64_t, uint64_t, uint64_t, const struct NileReply*);
  NileStatus (*emit)(uint64_t, uint64_t, uint64_t, const struct NileReply*);
  NileStatus (*resource_create)(uint64_t, uint64_t, uint64_t, NileResourceDrop, uint64_t*);
  NileStatus (*resource_get)(uint64_t, uint64_t, uint64_t, uint64_t*);
  NileStatus (*resource_release)(uint64_t, uint64_t, uint64_t);
} NileExtensionApi;

typedef struct NileExtension {
  uint32_t abi_version;
  uint32_t struct_size;
  uint64_t required_capabilities;
  uint64_t user;
  NileStatus (*context_init)(uint64_t, uint64_t);
  NileStatus (*context_dispose)(uint64_t, uint64_t);
  void (*cancel)(uint64_t, uint64_t, uint64_t);
  NileStatus (*shutdown)(uint64_t);
} NileExtension;

#define NILE_OK 0

#define NILE_INVALID 1

#define NILE_CLOSED 2

#define NILE_DUPLICATE 3

#define NILE_OVERLOADED 4

#define NILE_FAILED 5

#define NILE_CANCELLED 6

#define NILE_PANIC 7

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * The only symbol a native package must export. Tables are size/version checked.
 * On Windows the prototype and the definition must both be `dllexport`.
 */
NILE_EXTENSION_EXPORT NileStatus nile_extension_init(const struct NileExtensionApi *api,
                                      struct NileExtension *extension);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* NILE_EXTENSION_H */
