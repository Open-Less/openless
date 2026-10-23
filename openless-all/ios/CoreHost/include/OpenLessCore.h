#ifndef OPENLESS_IOS_CORE_H
#define OPENLESS_IOS_CORE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Callbacks may arrive on any thread. Pointers are BORROWED for the call only;
// copy before returning. Neither callback may synchronously destroy the host.
typedef void (*OLCoreEvent)(uint64_t context, const uint8_t *json, size_t length);
typedef void (*OLCorePCM)(uint64_t context, const char *session, const uint8_t *pcm, size_t length);
// Returns 0 on invalid config/initialization failure. Config: {"dataDir":"/..."}.
uint64_t ol_ios_create(const uint8_t *json, size_t length, OLCoreEvent event, OLCorePCM audio, uint64_t context);
int32_t ol_ios_command(uint64_t host, const uint8_t *json, size_t length);
int32_t ol_ios_reply(uint64_t host, const uint8_t *json, size_t length);
// Canonical 16kHz mono signed-16 little-endian PCM. Native input only.
int32_t ol_ios_feed_pcm(uint64_t host, const char *session, const uint8_t *pcm, size_t length);
// Native capture MUST already be stopped. Invalidates callbacks and joins
// Core cleanup. Call off the UI/audio threads. Repeated destroy is harmless.
void ol_ios_destroy(uint64_t host);
uint32_t ol_ios_contract_version(void);
#ifdef __cplusplus
}
#endif
#endif
