#include "registry.h"

#include <msctf.h>
#include <strsafe.h>

#include "guids.h"

namespace {

constexpr wchar_t kClsidKey[] =
    L"Software\\Classes\\CLSID\\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
constexpr wchar_t kInprocServer32Key[] =
    L"Software\\Classes\\CLSID\\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\\InprocServer32";
constexpr REGSAM kRegistryWriteAccess = KEY_WRITE;

HRESULT HResultFromWin32Error(LSTATUS status) {
  return status == ERROR_SUCCESS ? S_OK : HRESULT_FROM_WIN32(status);
}

HRESULT SetStringValue(HKEY key, const wchar_t* name, const wchar_t* value) {
  const auto byte_count =
      static_cast<DWORD>((wcslen(value) + 1) * sizeof(wchar_t));
  return HResultFromWin32Error(
      RegSetValueExW(key, name, 0, REG_SZ,
                     reinterpret_cast<const BYTE*>(value), byte_count));
}

HRESULT RegisterComServer(HINSTANCE module) {
  wchar_t module_path[MAX_PATH] = {};
  const DWORD path_len =
      GetModuleFileNameW(module, module_path, ARRAYSIZE(module_path));
  if (path_len == 0) {
    return HRESULT_FROM_WIN32(GetLastError());
  }
  if (path_len == ARRAYSIZE(module_path)) {
    return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
  }

  RegDeleteTreeW(HKEY_CURRENT_USER, kClsidKey);

  HKEY clsid_key = nullptr;
  LSTATUS status =
      RegCreateKeyExW(HKEY_LOCAL_MACHINE, kClsidKey, 0, nullptr,
                      REG_OPTION_NON_VOLATILE, kRegistryWriteAccess, nullptr,
                      &clsid_key, nullptr);
  HRESULT hr = HResultFromWin32Error(status);
  if (FAILED(hr)) {
    return hr;
  }

  hr = SetStringValue(clsid_key, nullptr, kOpenLessImeName);
  RegCloseKey(clsid_key);
  if (FAILED(hr)) {
    return hr;
  }

  HKEY inproc_key = nullptr;
  status = RegCreateKeyExW(HKEY_LOCAL_MACHINE, kInprocServer32Key, 0, nullptr,
                           REG_OPTION_NON_VOLATILE, kRegistryWriteAccess, nullptr,
                           &inproc_key, nullptr);
  hr = HResultFromWin32Error(status);
  if (FAILED(hr)) {
    return hr;
  }

  hr = SetStringValue(inproc_key, nullptr, module_path);
  if (SUCCEEDED(hr)) {
    hr = SetStringValue(inproc_key, L"ThreadingModel", L"Apartment");
  }
  RegCloseKey(inproc_key);
  return hr;
}

HRESULT DeleteComServerRegistration() {
  const LSTATUS user_status = RegDeleteTreeW(HKEY_CURRENT_USER, kClsidKey);
  if (user_status != ERROR_SUCCESS && user_status != ERROR_FILE_NOT_FOUND) {
    return HResultFromWin32Error(user_status);
  }

  const LSTATUS machine_status = RegDeleteTreeW(HKEY_LOCAL_MACHINE, kClsidKey);
  if (machine_status == ERROR_FILE_NOT_FOUND) {
    return S_OK;
  }
  return HResultFromWin32Error(machine_status);
}

class ScopedComInit {
 public:
  ScopedComInit() : hr_(CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED)) {}
  ScopedComInit(const ScopedComInit&) = delete;
  ScopedComInit& operator=(const ScopedComInit&) = delete;
  ~ScopedComInit() {
    if (hr_ == S_OK || hr_ == S_FALSE) {
      CoUninitialize();
    }
  }

  HRESULT hr() const {
    return hr_ == RPC_E_CHANGED_MODE ? S_OK : hr_;
  }

 private:
  HRESULT hr_;
};

HRESULT CreateProfiles(ITfInputProcessorProfiles** profiles) {
  return CoCreateInstance(CLSID_TF_InputProcessorProfiles, nullptr,
                          CLSCTX_INPROC_SERVER,
                          IID_ITfInputProcessorProfiles,
                          reinterpret_cast<void**>(profiles));
}

HRESULT CreateProfileManager(ITfInputProcessorProfileMgr** manager) {
  return CoCreateInstance(CLSID_TF_InputProcessorProfiles, nullptr,
                          CLSCTX_INPROC_SERVER,
                          IID_ITfInputProcessorProfileMgr,
                          reinterpret_cast<void**>(manager));
}

HRESULT CreateCategoryManager(ITfCategoryMgr** category_mgr) {
  return CoCreateInstance(CLSID_TF_CategoryMgr, nullptr, CLSCTX_INPROC_SERVER,
                          IID_ITfCategoryMgr,
                          reinterpret_cast<void**>(category_mgr));
}

LANGID ResolveOpenLessLangId() {
  const LANGID ui_lang = GetUserDefaultUILanguage();
  const WORD primary = PRIMARYLANGID(ui_lang);
  const WORD sub = SUBLANGID(ui_lang);

  if (primary == LANG_CHINESE) {
    if (sub == SUBLANG_CHINESE_TRADITIONAL || sub == SUBLANG_CHINESE_HONGKONG ||
        sub == SUBLANG_CHINESE_MACAU) {
      return kOpenLessLangIdTraditional;
    }
  }

  // Keep Simplified Chinese as the default fallback for non-Chinese locales,
  // so existing behavior remains stable outside zh-Hant systems.
  return kOpenLessLangIdSimplified;
}

HRESULT RegisterLanguageProfile() {
  ScopedComInit com;
  HRESULT hr = com.hr();
  if (FAILED(hr)) {
    return hr;
  }

  ITfInputProcessorProfiles* profiles = nullptr;
  hr = CreateProfiles(&profiles);
  if (FAILED(hr)) {
    return hr;
  }

  const LANGID openless_lang_id = ResolveOpenLessLangId();

  profiles->Unregister(CLSID_OpenLessTextService);

  hr = profiles->Register(CLSID_OpenLessTextService);
  if (SUCCEEDED(hr)) {
    hr = profiles->AddLanguageProfile(
        CLSID_OpenLessTextService, openless_lang_id, GUID_OpenLessProfile,
        const_cast<wchar_t*>(kOpenLessImeName),
        static_cast<ULONG>(ARRAYSIZE(kOpenLessImeName) - 1), nullptr, 0, 0);
  }
  if (SUCCEEDED(hr)) {
    hr = profiles->EnableLanguageProfile(CLSID_OpenLessTextService,
                                         openless_lang_id, GUID_OpenLessProfile,
                                         TRUE);
  }

  profiles->Release();

  if (FAILED(hr)) {
    return hr;
  }

  ITfInputProcessorProfileMgr* manager = nullptr;
  hr = CreateProfileManager(&manager);
  if (FAILED(hr)) {
    return hr;
  }

  manager->UnregisterProfile(CLSID_OpenLessTextService, kOpenLessLangIdSimplified,
                             GUID_OpenLessProfile, 0);
  manager->UnregisterProfile(CLSID_OpenLessTextService, kOpenLessLangIdTraditional,
                             GUID_OpenLessProfile, 0);
  hr = manager->RegisterProfile(
      CLSID_OpenLessTextService, openless_lang_id, GUID_OpenLessProfile,
      kOpenLessImeName, static_cast<ULONG>(ARRAYSIZE(kOpenLessImeName) - 1),
      nullptr, 0, 0, nullptr, 0, TRUE,
      TF_IPP_CAPS_IMMERSIVESUPPORT | TF_IPP_CAPS_SYSTRAYSUPPORT);
  manager->Release();
  return hr;
}

HRESULT RegisterKeyboardCategory() {
  ScopedComInit com;
  HRESULT hr = com.hr();
  if (FAILED(hr)) {
    return hr;
  }

  ITfCategoryMgr* category_mgr = nullptr;
  hr = CreateCategoryManager(&category_mgr);
  if (FAILED(hr)) {
    return hr;
  }

  category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                   GUID_TFCAT_TIP_KEYBOARD,
                                   CLSID_OpenLessTextService);
  category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                   GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                                   CLSID_OpenLessTextService);
  category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                   GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                                   CLSID_OpenLessTextService);

  hr = category_mgr->RegisterCategory(CLSID_OpenLessTextService,
                                      GUID_TFCAT_TIP_KEYBOARD,
                                      CLSID_OpenLessTextService);
  if (SUCCEEDED(hr)) {
    hr = category_mgr->RegisterCategory(CLSID_OpenLessTextService,
                                        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                                        CLSID_OpenLessTextService);
  }
  if (SUCCEEDED(hr)) {
    hr = category_mgr->RegisterCategory(CLSID_OpenLessTextService,
                                        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                                        CLSID_OpenLessTextService);
  }
  category_mgr->Release();
  return hr;
}

HRESULT UnregisterLanguageProfile() {
  ScopedComInit com;
  HRESULT hr = com.hr();
  if (FAILED(hr)) {
    return hr;
  }

  ITfInputProcessorProfiles* profiles = nullptr;
  hr = CreateProfiles(&profiles);
  if (FAILED(hr)) {
    return hr;
  }

  ITfInputProcessorProfileMgr* manager = nullptr;
  hr = CreateProfileManager(&manager);
  if (SUCCEEDED(hr)) {
    manager->UnregisterProfile(CLSID_OpenLessTextService, kOpenLessLangIdSimplified,
                               GUID_OpenLessProfile, 0);
    manager->UnregisterProfile(CLSID_OpenLessTextService, kOpenLessLangIdTraditional,
                               GUID_OpenLessProfile, 0);
    manager->Release();
  }

  hr = profiles->Unregister(CLSID_OpenLessTextService);
  profiles->Release();
  return hr;
}

HRESULT UnregisterKeyboardCategory() {
  ScopedComInit com;
  HRESULT hr = com.hr();
  if (FAILED(hr)) {
    return hr;
  }

  ITfCategoryMgr* category_mgr = nullptr;
  hr = CreateCategoryManager(&category_mgr);
  if (FAILED(hr)) {
    return hr;
  }

  hr = category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                        GUID_TFCAT_TIP_KEYBOARD,
                                        CLSID_OpenLessTextService);
  if (SUCCEEDED(hr)) {
    hr = category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                          GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                                          CLSID_OpenLessTextService);
  }
  if (SUCCEEDED(hr)) {
    hr = category_mgr->UnregisterCategory(CLSID_OpenLessTextService,
                                          GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                                          CLSID_OpenLessTextService);
  }
  category_mgr->Release();
  return hr;
}

}  // namespace

HRESULT RegisterOpenLessTextService(HINSTANCE module) {
  if (module == nullptr) {
    return E_INVALIDARG;
  }

  HRESULT hr = RegisterComServer(module);
  if (FAILED(hr)) {
    return hr;
  }

  hr = RegisterLanguageProfile();
  if (FAILED(hr)) {
    DeleteComServerRegistration();
    return hr;
  }

  hr = RegisterKeyboardCategory();
  if (FAILED(hr)) {
    UnregisterLanguageProfile();
    DeleteComServerRegistration();
  }
  return hr;
}

HRESULT UnregisterOpenLessTextService() {
  const HRESULT category_hr = UnregisterKeyboardCategory();
  const HRESULT profile_hr = UnregisterLanguageProfile();
  const HRESULT registry_hr = DeleteComServerRegistration();
  if (FAILED(category_hr)) {
    return category_hr;
  }
  return FAILED(profile_hr) ? profile_hr : registry_hr;
}
