#pragma once
// Returned UTF-8 JSON is owned by the caller and freed with openless_rime_free.
extern "C" char* openless_rime_request(const char* operation, const char* session, const char* argument);
extern "C" void openless_rime_free(char* response);
