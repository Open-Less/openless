#pragma once

#include <msctf.h>
#include <windows.h>

inline constexpr CLSID CLSID_OpenLessTextService = {
    0x6b9f3f4f,
    0x5ee7,
    0x42d6,
    {0x9c, 0x61, 0x9f, 0x80, 0xb0, 0x3a, 0x5d, 0x7d},
};

inline constexpr GUID GUID_OpenLessProfile = {
    0x9b5f5e04,
    0x23f6,
    0x47da,
    {0x9a, 0x26, 0xd2, 0x21, 0xf6, 0xc3, 0xf0, 0x2e},
};

inline constexpr wchar_t kOpenLessImeName[] = L"OpenLess Voice Input";
inline constexpr LANGID kOpenLessLangIdSimplified = 0x0804;
inline constexpr LANGID kOpenLessLangIdTraditional = 0x0404;
