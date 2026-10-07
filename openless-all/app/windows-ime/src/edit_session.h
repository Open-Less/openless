#pragma once

#include <msctf.h>
#include <memory>
#include <string>
#include <windows.h>

// Shared between the text service and an async edit session. Both only touch
// it on the TSF owner thread, so plain fields are enough.
struct OpenLessAsyncEditState {
  bool cancelled = false;
  bool completed = false;
  HRESULT result = E_PENDING;
};

class OpenLessEditSession final : public ITfEditSession {
public:
  OpenLessEditSession(ITfContext *context, std::wstring text,
                      std::shared_ptr<OpenLessAsyncEditState> async_state = nullptr);
  OpenLessEditSession(const OpenLessEditSession &) = delete;
  OpenLessEditSession &operator=(const OpenLessEditSession &) = delete;
  ~OpenLessEditSession();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP DoEditSession(TfEditCookie edit_cookie) override;

private:
  HRESULT InsertText(TfEditCookie edit_cookie);

  LONG ref_count_ = 1;
  ITfContext *context_ = nullptr;
  std::wstring text_;
  std::shared_ptr<OpenLessAsyncEditState> async_state_;
};
