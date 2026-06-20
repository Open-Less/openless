#include <windows.h>

#include <new>

#include "class_factory.h"
#include "guids.h"
#include "registry.h"

HINSTANCE g_module = nullptr;
LONG g_lock_count = 0;
LONG g_object_count = 0;

namespace {

bool IsBlockedShellProcess() {
  wchar_t path[MAX_PATH] = {};
  const DWORD length = GetModuleFileNameW(nullptr, path, ARRAYSIZE(path));
  if (length == 0 || length >= ARRAYSIZE(path)) {
    return false;
  }

  const wchar_t* name = path;
  for (const wchar_t* cursor = path; *cursor != L'\0'; ++cursor) {
    if (*cursor == L'\\' || *cursor == L'/') {
      name = cursor + 1;
    }
  }

  constexpr const wchar_t* kBlockedProcesses[] = {
      L"explorer.exe",
      L"searchhost.exe",
      L"startmenuexperiencehost.exe",
      L"shellexperiencehost.exe",
      L"textinputhost.exe",
  };
  for (const wchar_t* blocked : kBlockedProcesses) {
    if (_wcsicmp(name, blocked) == 0) {
      return true;
    }
  }
  return false;
}

}  // namespace

BOOL APIENTRY DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
  UNREFERENCED_PARAMETER(reserved);

  // Do not call DisableThreadLibraryCalls here. OpenLessIme is linked with the
  // static CRT, and suppressing thread attach/detach callbacks can break CRT
  // thread-local cleanup in host input processes.
  if (reason == DLL_PROCESS_ATTACH) {
    if (IsBlockedShellProcess()) {
      return FALSE;
    }
    g_module = instance;
  }

  return TRUE;
}

STDAPI DllCanUnloadNow() {
  return (g_lock_count == 0 && g_object_count == 0) ? S_OK : S_FALSE;
}

STDAPI DllGetClassObject(REFCLSID clsid, REFIID iid, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (clsid != CLSID_OpenLessTextService) {
    return CLASS_E_CLASSNOTAVAILABLE;
  }

  if (IsBlockedShellProcess()) {
    return CLASS_E_CLASSNOTAVAILABLE;
  }

  auto* factory = new (std::nothrow) OpenLessClassFactory();
  if (factory == nullptr) {
    return E_OUTOFMEMORY;
  }

  const HRESULT hr = factory->QueryInterface(iid, object);
  factory->Release();
  return hr;
}

STDAPI DllRegisterServer() {
  return RegisterOpenLessTextService(g_module);
}

STDAPI DllUnregisterServer() {
  return UnregisterOpenLessTextService();
}
