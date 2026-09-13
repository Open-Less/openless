#include "class_factory.h"

#include <new>
#include <windows.h>

#include "text_service.h"

extern LONG g_lock_count;
extern LONG g_object_count;

OpenLessClassFactory::OpenLessClassFactory() { InterlockedIncrement(&g_object_count); }

OpenLessClassFactory::~OpenLessClassFactory() { InterlockedDecrement(&g_object_count); }

STDMETHODIMP OpenLessClassFactory::QueryInterface(REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (iid == IID_IUnknown || iid == IID_IClassFactory) {
    *object = static_cast<IClassFactory *>(this);
    AddRef();
    return S_OK;
  }

  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenLessClassFactory::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessClassFactory::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessClassFactory::CreateInstance(IUnknown *outer, REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (outer != nullptr) {
    return CLASS_E_NOAGGREGATION;
  }

  auto *service = new (std::nothrow) OpenLessTextService();
  if (service == nullptr) {
    return E_OUTOFMEMORY;
  }

  const HRESULT hr = service->QueryInterface(iid, object);
  service->Release();
  return hr;
}

STDMETHODIMP OpenLessClassFactory::LockServer(BOOL lock) {
  if (lock) {
    InterlockedIncrement(&g_lock_count);
  } else {
    InterlockedDecrement(&g_lock_count);
  }
  return S_OK;
}
