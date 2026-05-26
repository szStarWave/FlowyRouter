#ifndef FLOWY_ROUTER_H
#define FLOWY_ROUTER_H

#include <stddef.h>
#include <stdint.h>

#ifdef _WIN32
#  ifdef FLOWY_ROUTER_EXPORTS
#    define FLOWY_ROUTER_API __declspec(dllexport)
#  else
#    define FLOWY_ROUTER_API __declspec(dllimport)
#  endif
#else
#  define FLOWY_ROUTER_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define FLOWY_OK 0
#define FLOWY_ERR_ALREADY_RUNNING 1
#define FLOWY_ERR_NOT_RUNNING 2
#define FLOWY_ERR_INVALID_ARG 3
#define FLOWY_ERR_INTERNAL 4

FLOWY_ROUTER_API const char *flowy_router_version(void);

FLOWY_ROUTER_API int32_t flowy_router_start(
    const char *config_path,
    char *error_out,
    size_t error_out_len);

FLOWY_ROUTER_API int32_t flowy_router_stop(char *error_out, size_t error_out_len);

FLOWY_ROUTER_API int32_t flowy_router_is_running(void);

FLOWY_ROUTER_API int32_t flowy_router_gateway_url(char *url_out, size_t url_out_len);

#ifdef __cplusplus
}
#endif

#endif /* FLOWY_ROUTER_H */
