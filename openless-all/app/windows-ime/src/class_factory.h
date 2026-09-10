#pragma once

#include <unknwn.h>

class OpenLessClassFactory final : public IClassFactory {
public:
  OpenLessClassFactory();
  OpenLessClassFactory(const OpenLessClassFactory &) = delete;
  OpenLessClassFactory &operator=(const OpenLessClassFactory &) = delete;
  ~OpenLessClassFactory();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP CreateInstance(IUnknown *outer, REFIID iid, void **object) override;
  STDMETHODIMP LockServer(BOOL lock) override;

private:
  LONG ref_count_ = 1;
};
